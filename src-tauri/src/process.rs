//! The single entry point for running external programs (`git`, `gh`, package managers).
//! Arguments are always passed as a vector, never through a shell.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::platform;

#[derive(Debug, Clone)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program` to completion and returns its trimmed output, or `CommandFailed` on a
/// non-zero exit.
pub async fn run<I, S>(program: &str, args: I, cwd: Option<&Path>) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = checked_args(program, args)?;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Error::NotInstalled {
                program: program.to_owned(),
            });
        }
        Err(err) => return Err(err.into()),
    };

    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr)
        .trim_end()
        .to_owned();
    if !output.status.success() {
        return Err(Error::CommandFailed {
            program: program.to_owned(),
            code: output.status.code(),
            stderr,
        });
    }
    Ok(Output { stdout, stderr })
}

/// How `run_streaming` runs a long command (package managers, `gh extension install`).
pub struct Streaming<'a> {
    /// Extra environment for the child (e.g. Homebrew's `HOMEBREW_NO_AUTO_UPDATE`).
    pub envs: &'a [(&'a str, &'a str)],
    /// Killed with `Error::TimedOut` when it runs longer than this.
    pub timeout: Duration,
}

/// How many output lines a failure keeps for its message.
const TAIL_LINES: usize = 12;
/// How long the output may keep flowing after the program exited: a grandchild that
/// inherited the pipes (a daemon, a helper) would otherwise keep them open forever.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Runs `program` like `run`, but hands each output line (stdout and stderr, `\r` counts as a
/// line end, escape codes removed) to `on_line` as it arrives, gives up after
/// `opts.timeout` (`Error::TimedOut`) and stops when `cancel` resolves (`Error::Cancelled`).
/// Either way the program, and on Unix the processes it started, are killed. A failure
/// carries the last lines of output (stderr when it printed any).
pub async fn run_streaming<I, S, C, F>(
    program: &str,
    args: I,
    opts: Streaming<'_>,
    cancel: C,
    mut on_line: F,
) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    C: Future<Output = ()>,
    F: FnMut(&str),
{
    let args = checked_args(program, args)?;
    let mut cmd = command(program);
    cmd.args(args)
        .envs(opts.envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Its own process group, so a kill reaches what it started (brew runs ruby, curl...).
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Error::NotInstalled {
                program: program.to_owned(),
            });
        }
        Err(err) => return Err(err.into()),
    };
    // Read now: tokio forgets the id once the child has been waited on.
    let pid = child.id();

    let (tx, mut rx) = mpsc::unbounded_channel::<(bool, String)>();
    let readers = [
        child
            .stdout
            .take()
            .map(|out| read_lines(out, false, tx.clone())),
        child
            .stderr
            .take()
            .map(|err| read_lines(err, true, tx.clone())),
    ];
    drop(tx);

    let mut tail = Tail::default();
    let deadline = tokio::time::sleep(opts.timeout);
    tokio::pin!(deadline, cancel);
    let mut lines_open = true;
    let status = loop {
        tokio::select! {
            line = rx.recv(), if lines_open => match line {
                Some((is_err, line)) => {
                    tail.push(is_err, &line);
                    on_line(&line);
                }
                None => lines_open = false,
            },
            status = child.wait() => break Some(status?),
            () = &mut deadline => break None,
            () = &mut cancel => {
                kill(&mut child, pid).await;
                abort(&readers);
                return Err(Error::Cancelled);
            }
        }
    };
    let Some(status) = status else {
        kill(&mut child, pid).await;
        abort(&readers);
        return Err(Error::TimedOut {
            program: program.to_owned(),
            minutes: opts.timeout.as_millis().div_ceil(60_000) as u64,
            output: tail.text(),
        });
    };

    // Whatever is still in the pipes, unless something else holds them open.
    let drain = tokio::time::sleep(DRAIN_GRACE);
    tokio::pin!(drain);
    while lines_open {
        tokio::select! {
            line = rx.recv() => match line {
                Some((is_err, line)) => {
                    tail.push(is_err, &line);
                    on_line(&line);
                }
                None => lines_open = false,
            },
            () = &mut drain => break,
        }
    }
    abort(&readers);

    if !status.success() {
        return Err(Error::CommandFailed {
            program: program.to_owned(),
            code: status.code(),
            stderr: tail.text(),
        });
    }
    Ok(())
}

fn read_lines<R>(
    mut pipe: R,
    is_err: bool,
    tx: mpsc::UnboundedSender<(bool, String)>,
) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = LineSplitter::default();
        let mut buf = [0u8; 4096];
        loop {
            let n = match pipe.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for line in lines.push(&buf[..n]) {
                if tx.send((is_err, line)).is_err() {
                    return;
                }
            }
        }
        if let Some(line) = lines.finish() {
            let _ = tx.send((is_err, line));
        }
    })
}

fn abort(readers: &[Option<tokio::task::JoinHandle<()>>]) {
    for reader in readers.iter().flatten() {
        reader.abort();
    }
}

/// Kills the program and, on Unix, its process group. A program that became root
/// (`pkexec`) can't be killed; it is left to finish on its own.
async fn kill(child: &mut tokio::process::Child, pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        kill_group(pid);
    }
    #[cfg(not(unix))]
    let _ = pid;
    let _ = child.kill().await;
}

/// The group's id is the program's pid (`process_group(0)`).
#[cfg(unix)]
fn kill_group(pid: u32) {
    // `kill` is a coreutil everywhere; no libc dependency for one call.
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Splits output into lines on `\n` or `\r` (progress bars redraw with `\r`), drops escape
/// codes and blank or spinner-only lines.
#[derive(Default)]
struct LineSplitter {
    pending: Vec<u8>,
}

impl LineSplitter {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' || byte == b'\r' {
                let raw = std::mem::take(&mut self.pending);
                lines.extend(clean_line(&raw));
            } else {
                self.pending.push(byte);
            }
        }
        lines
    }

    fn finish(&mut self) -> Option<String> {
        let raw = std::mem::take(&mut self.pending);
        clean_line(&raw)
    }
}

/// Removes ANSI escape sequences and trims; `None` when nothing readable is left (blank
/// lines, a lone spinner character).
fn clean_line(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // CSI: parameters, then one final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    let line = out.trim();
    (line.chars().filter(|c| c.is_alphanumeric()).count() > 0).then(|| line.to_owned())
}

/// The last output lines, stderr's kept apart: it holds the error when there is one.
#[derive(Default)]
struct Tail {
    out: VecDeque<String>,
    err: VecDeque<String>,
}

impl Tail {
    fn push(&mut self, is_err: bool, line: &str) {
        let lines = if is_err { &mut self.err } else { &mut self.out };
        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line.to_owned());
    }

    fn text(&self) -> String {
        let lines = if self.err.is_empty() {
            &self.out
        } else {
            &self.err
        };
        lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Starts `program` with stdout/stderr piped, for long-running commands whose output is
/// streamed (e.g. `gh auth login`). The child is killed if the handle is dropped.
pub fn spawn_piped<I, S>(program: &str, args: I) -> Result<tokio::process::Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = checked_args(program, args)?;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(Error::NotInstalled {
            program: program.to_owned(),
        }),
        Err(err) => Err(err.into()),
    }
}

/// Starts `program` without waiting for it (terminals, editors).
pub fn spawn_detached<I, S>(program: &str, args: I, cwd: Option<&Path>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = checked_args(program, args)?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .env("PATH", search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(Error::NotInstalled {
            program: program.to_owned(),
        }),
        Err(err) => Err(err.into()),
    }
}

/// Development builds run against the developer's real classes, so they never write to
/// GitHub: submitting, accepting, inviting, joining a team and `git push` are refused.
fn checked_args<I, S>(program: &str, args: I) -> Result<Vec<OsString>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_owned()).collect();
    if cfg!(debug_assertions) && writes_to_github(program, &args) {
        let line = std::iter::once(program.to_owned())
            .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        return Err(Error::BlockedInDev(line));
    }
    Ok(args)
}

pub(crate) fn writes_to_github(program: &str, args: &[OsString]) -> bool {
    let words: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().to_ascii_lowercase())
        .filter(|a| !a.starts_with('-'))
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let name = Path::new(program)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase());
    match (name.as_deref(), words.as_slice()) {
        (Some("gh"), ["student", "submit" | "accept" | "invite", ..]) => true,
        (Some("gh"), ["student", "team", "add", ..]) => true,
        (Some("git"), words) => words.contains(&"push"),
        _ => false,
    }
}

/// Looks `program` up on the augmented search path, like `which`.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), format!("{program}.cmd")]
    } else {
        vec![program.to_owned()]
    };
    std::env::split_paths(&search_path()).find_map(|dir| {
        candidates
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PATH", search_path())
        // gh must never block on an interactive prompt or print an upgrade banner.
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        // Nor git (submit pushes with the user's own credentials): fail instead of hanging.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// GUI apps don't inherit the login shell's PATH (a Finder-launched macOS app has no
/// `/opt/homebrew/bin`), so the inherited PATH is extended with the usual install locations.
/// Built again for each program: a tool installed while the app runs (on Windows its folder
/// only lands in the registry) is found without a restart.
fn search_path() -> OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for extra in platform::extra_path_dirs() {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(program: &str, args: &[&str]) -> bool {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        writes_to_github(program, &args)
    }

    #[test]
    fn spots_commands_that_write_to_github() {
        assert!(writes("gh", &["student", "submit"]));
        assert!(writes(
            "gh",
            &["student", "accept", "cs50", "fall", "hello"]
        ));
        assert!(writes(
            "/opt/homebrew/bin/gh",
            &["student", "team", "add", "x"]
        ));
        assert!(writes("git", &["-C", "/x", "push", "origin", "main"]));
        assert!(!writes("gh", &["student", "whoami"]));
        assert!(!writes("gh", &["student", "team", "list"]));
        assert!(!writes("git", &["--no-optional-locks", "status"]));
        assert!(!writes("gh", &["auth", "token"]));
    }

    fn opts(timeout: Duration) -> Streaming<'static> {
        Streaming {
            envs: &[("C50_TEST", "from-env")],
            timeout,
        }
    }

    fn never() -> std::future::Pending<()> {
        std::future::pending()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn streams_lines_as_they_arrive() {
        let start = std::time::Instant::now();
        let mut seen = Vec::new();
        let script = "echo one; echo \"$C50_TEST\" >&2; sleep 1; printf 'two'";
        run_streaming(
            "sh",
            ["-c", script],
            opts(Duration::from_secs(10)),
            never(),
            |line| seen.push((line.to_owned(), start.elapsed())),
        )
        .await
        .unwrap();
        let lines: Vec<&str> = seen.iter().map(|(l, _)| l.as_str()).collect();
        assert!(
            lines.contains(&"one") && lines.contains(&"from-env"),
            "{lines:?}"
        );
        assert_eq!(lines.last(), Some(&"two"));
        // `one` came before the program finished, not with everything at the end.
        let one = seen.iter().find(|(l, _)| l == "one").unwrap().1;
        assert!(one < Duration::from_millis(900), "{one:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failure_keeps_the_end_of_stderr() {
        let script = "echo progress; for i in $(seq 1 20); do echo err$i >&2; done; exit 3";
        let err = run_streaming(
            "sh",
            ["-c", script],
            opts(Duration::from_secs(10)),
            never(),
            |_| {},
        )
        .await
        .unwrap_err();
        let Error::CommandFailed { code, stderr, .. } = err else {
            panic!("{err:?}");
        };
        assert_eq!(code, Some(3));
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), TAIL_LINES);
        assert_eq!(lines.first(), Some(&"err9"));
        assert_eq!(lines.last(), Some(&"err20"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failure_without_stderr_keeps_stdout() {
        let err = run_streaming(
            "sh",
            ["-c", "echo only-stdout; exit 1"],
            opts(Duration::from_secs(10)),
            never(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, Error::CommandFailed { stderr, .. } if stderr == "only-stdout"),
            "{err:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn times_out_with_the_output_so_far() {
        let start = std::time::Instant::now();
        let err = run_streaming(
            "sh",
            ["-c", "echo waiting; sleep 30"],
            opts(Duration::from_millis(500)),
            never(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(
            matches!(&err, Error::TimedOut { output, minutes: 1, .. } if output == "waiting"),
            "{err:?}"
        );
    }

    /// The sleep is a grandchild holding the pipes: cancelling kills the whole group.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_kills_the_program_and_what_it_started() {
        let marker = std::env::temp_dir().join(format!("c50-cancel-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!(
            "(sleep 2; touch '{}') & echo started; wait",
            marker.display()
        );
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut tx = Some(tx);
        let start = std::time::Instant::now();
        let err = run_streaming(
            "sh",
            ["-c", script.as_str()],
            opts(Duration::from_secs(30)),
            async {
                let _ = rx.await;
            },
            |line| {
                if line == "started"
                    && let Some(tx) = tx.take()
                {
                    let _ = tx.send(());
                }
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{err:?}");
        assert!(start.elapsed() < Duration::from_secs(2));
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!marker.exists(), "the grandchild survived the cancel");
    }

    /// A background process that keeps the pipes open must not hold the result back.
    #[cfg(unix)]
    #[tokio::test]
    async fn returns_when_the_program_exits_even_if_its_pipes_stay_open() {
        let start = std::time::Instant::now();
        run_streaming(
            "sh",
            ["-c", "sleep 20 & echo done"],
            opts(Duration::from_secs(30)),
            never(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn a_missing_program_is_not_installed() {
        let err = run_streaming(
            "c50-no-such-program",
            ["x"],
            opts(Duration::from_secs(1)),
            never(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::NotInstalled { .. }), "{err:?}");
    }

    #[test]
    fn splits_lines_on_newlines_and_carriage_returns() {
        let mut lines = LineSplitter::default();
        assert_eq!(
            lines.push(b"Downloading 10%\rDownloading 50%\r"),
            ["Downloading 10%", "Downloading 50%"]
        );
        assert_eq!(lines.push(b"Instal"), Vec::<String>::new());
        assert_eq!(lines.push(b"led\r\n\n  \n"), ["Installed"]);
        assert_eq!(lines.push(b"tail"), Vec::<String>::new());
        assert_eq!(lines.finish().as_deref(), Some("tail"));
        assert_eq!(lines.finish(), None);
    }

    #[test]
    fn cleans_escape_codes_and_spinners() {
        assert_eq!(
            clean_line(b"\x1b[1;32m==>\x1b[0m Pouring gh").as_deref(),
            Some("==> Pouring gh")
        );
        assert_eq!(clean_line(b"\x1b[2K  -  "), None);
        assert_eq!(clean_line("\u{2588}\u{2588}\u{2592}".as_bytes()), None);
        assert_eq!(
            clean_line("  \u{2588}\u{2588} 1.2 MB / 9 MB".as_bytes()).as_deref(),
            Some("\u{2588}\u{2588} 1.2 MB / 9 MB")
        );
    }

    #[test]
    fn dev_builds_refuse_submit() {
        let result = checked_args("gh", ["student", "submit"]);
        assert_eq!(
            cfg!(debug_assertions),
            matches!(result, Err(Error::BlockedInDev(_)))
        );
    }
}
