//! Each clone's state from git (status, unmerged branches, GitHub's head), read without
//! writing to it, and teammates' commit emails.

use std::path::{Path, PathBuf};

use tokio::task::JoinSet;

use super::{LocalState, MAX_CHANGED_PATHS, Progress, Project};
use crate::groups;
use crate::noise;
use crate::process;
use crate::sync_progress::Phase;

/// Teammates without a public email on GitHub get the one they commit with in the shared
/// clone (read from the local history, no request). GitHub's `noreply` addresses don't count.
pub(super) async fn commit_emails(projects: &mut [Project]) {
    for project in projects {
        let (Some(path), Some(group)) = (project.path.clone(), project.group.as_mut()) else {
            continue;
        };
        if group.members.iter().all(|m| m.email.is_some()) {
            continue;
        }
        let args = ["log", "--no-merges", "-n", "500", "--format=%an%x1f%ae"];
        let Ok(out) = process::run("git", args, Some(&path)).await else {
            continue;
        };
        for member in group.members.iter_mut().filter(|m| m.email.is_none()) {
            member.email = author_email(&out.stdout, member);
        }
    }
}

/// The newest commit email of `member`: an author named like their login or GitHub name, or
/// an address whose local part is their login.
fn author_email(log: &str, member: &groups::Member) -> Option<String> {
    log.lines()
        .filter_map(|line| line.split_once('\x1f'))
        .find(|(name, email)| {
            let name = name.trim();
            let local = email.split('@').next().unwrap_or("");
            email.contains('@')
                && !email.ends_with("@users.noreply.github.com")
                && (name.eq_ignore_ascii_case(&member.login)
                    || member
                        .name
                        .as_deref()
                        .is_some_and(|n| n.trim().eq_ignore_ascii_case(name))
                    || local.eq_ignore_ascii_case(&member.login))
        })
        .map(|(_, email)| email.trim().to_owned())
}

/// `git status` of each clone, concurrently, in input order.
pub(super) async fn local_states(
    paths: impl Iterator<Item = PathBuf>,
    progress: &Progress,
) -> Vec<Option<LocalState>> {
    let mut tasks = JoinSet::new();
    let mut count = 0;
    for (i, path) in paths.enumerate() {
        tasks.spawn(async move { (i, local_state(&path).await) });
        count += 1;
    }
    progress.set_total(Phase::Clones, count);
    let mut states = vec![None; count];
    while let Some(Ok((i, state))) = tasks.join_next().await {
        progress.advance(Phase::Clones);
        states[i] = state;
    }
    states
}

/// `git status` without taking the index lock, so it never races the student's own git.
/// Untracked editor, OS and build files (`noise`) don't count. A folder git collapsed into
/// one untracked entry is listed file by file once more, so a new folder holding only noise
/// (a `.DS_Store`) doesn't count either.
async fn local_state(path: &Path) -> Option<LocalState> {
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
    ];
    let out = match process::run("git", args, Some(path)).await {
        Ok(out) => out.stdout,
        Err(err) => {
            log::debug!("git status in {}: {err}", path.display());
            return None;
        }
    };
    let tree = noise::OnDisk::new(path);
    let entries = status_entries(&out);
    let folders: Vec<&str> = entries
        .iter()
        .filter(|e| e.untracked && e.path.ends_with('/') && !noise::is_noise(e.path, &tree))
        .map(|e| e.path)
        .collect();
    let mut real_folders = std::collections::HashSet::new();
    if !folders.is_empty() {
        let args = [
            "--literal-pathspecs",
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--",
        ]
        .into_iter()
        .chain(folders.iter().copied());
        match process::run("git", args, Some(path)).await {
            Ok(inner) => {
                for entry in status_entries(&inner.stdout) {
                    if entry.untracked
                        && !noise::is_noise(entry.path, &tree)
                        && let Some(folder) = folders.iter().find(|f| entry.path.starts_with(**f))
                    {
                        real_folders.insert(*folder);
                    }
                }
            }
            // Can't tell: the folders count, as git reported them.
            Err(_) => real_folders.extend(folders.iter().copied()),
        }
    }
    Some(parse_status(&out, |entry| {
        noise::is_noise(entry, &tree) || entry.ends_with('/') && !real_folders.contains(entry)
    }))
}

/// Compares each clone with GitHub's data: unmerged local branches, and whether the
/// remote head is missing (needs a pull). No network: only the local object store.
pub(super) async fn check_clones(projects: &mut [Project]) {
    let mut tasks = JoinSet::new();
    for (i, project) in projects.iter().enumerate() {
        let (Some(path), Some(local)) = (project.path.clone(), project.local.as_ref()) else {
            continue;
        };
        let remote = project.remote.as_ref();
        let default_branch = remote.and_then(|r| r.default_branch.clone());
        let remote_head = remote
            .and_then(|r| r.head.as_ref())
            .map(|h| h.oid.clone())
            .filter(|oid| local.head.as_deref() != Some(oid.as_str()));
        tasks.spawn(async move {
            let unmerged = match default_branch {
                Some(branch) => unmerged_branches(&path, &branch).await,
                None => Vec::new(),
            };
            let remote_ahead = match remote_head {
                Some(oid) => !has_commit(&path, &oid).await,
                None => false,
            };
            (i, unmerged, remote_ahead)
        });
    }
    while let Some(Ok((i, unmerged, remote_ahead))) = tasks.join_next().await {
        if let Some(local) = projects[i].local.as_mut() {
            local.unmerged = unmerged;
            local.remote_ahead = remote_ahead;
        }
    }
}

/// Local branches with commits not on `default` (the feedback branch never counts).
async fn unmerged_branches(path: &Path, default: &str) -> Vec<String> {
    let no_merged = format!("--no-merged=refs/heads/{default}");
    let args = [
        "for-each-ref",
        no_merged.as_str(),
        "--format=%(refname:short)",
        "refs/heads",
    ];
    match process::run("git", args, Some(path)).await {
        Ok(out) => out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|b| !b.is_empty() && *b != default && *b != "feedback")
            .map(str::to_owned)
            .collect(),
        // No local default branch yet: nothing to compare with.
        Err(_) => Vec::new(),
    }
}

async fn has_commit(path: &Path, oid: &str) -> bool {
    let object = format!("{oid}^{{commit}}");
    process::run("git", ["cat-file", "-e", object.as_str()], Some(path))
        .await
        .is_ok()
}

/// One entry of `git status --porcelain=v2 -z`: headers (`# ...`) aside.
struct StatusEntry<'a> {
    path: &'a str,
    untracked: bool,
}

/// The changed and untracked paths of `git status --porcelain=v2 -z` (ignored ones, `!`,
/// skipped). A rename's original path, the entry after it, is skipped too.
fn status_entries(porcelain: &str) -> Vec<StatusEntry<'_>> {
    let mut entries = Vec::new();
    let mut fields = porcelain.split('\0');
    while let Some(entry) = fields.next() {
        // Ordinary, renamed or copied, unmerged: the path follows a fixed count of fields.
        let (path, untracked) = match entry.split_at_checked(2) {
            Some(("1 ", _)) => (entry.splitn(9, ' ').nth(8), false),
            Some(("2 ", _)) => {
                fields.next();
                (entry.splitn(10, ' ').nth(9), false)
            }
            Some(("u ", _)) => (entry.splitn(11, ' ').nth(10), false),
            Some(("? ", path)) => (Some(path), true),
            _ => (None, false),
        };
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            entries.push(StatusEntry { path, untracked });
        }
    }
    entries
}

/// Reads `git status --porcelain=v2 -z --branch`. The untracked entries `is_noise` names
/// (noise files, folders holding only noise) don't count.
fn parse_status(porcelain: &str, is_noise: impl Fn(&str) -> bool) -> LocalState {
    let mut state = LocalState::default();
    for header in porcelain.split('\0').filter_map(|e| e.strip_prefix("# ")) {
        let (name, value) = header.split_once(' ').unwrap_or((header, ""));
        match name {
            "branch.oid" if value != "(initial)" => state.head = Some(value.to_owned()),
            "branch.head" if value != "(detached)" => state.branch = Some(value.to_owned()),
            "branch.ab" => {
                let mut counts = value
                    .split(' ')
                    .map(|c| c.trim_start_matches(['+', '-']).parse::<u32>().ok());
                state.ahead = counts.next().flatten();
                state.behind = counts.next().flatten();
            }
            _ => {}
        }
    }
    for entry in status_entries(porcelain) {
        state.tracked_changes += u32::from(!entry.untracked);
        let counts = !entry.untracked || !is_noise(entry.path);
        if counts {
            state.changed_files += 1;
            if state.changed_paths.len() < MAX_CHANGED_PATHS {
                state.changed_paths.push(entry.path.to_owned());
            }
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_teammates_commit_emails() {
        let log = "Bob B\x1f12+bob@users.noreply.github.com\nBob B\x1fbob@heig-vd.ch\nalice\x1falice@x.ch\n";
        let member = |login: &str, name: Option<&str>| groups::Member {
            login: login.into(),
            name: name.map(Into::into),
            email: None,
        };
        assert_eq!(
            author_email(log, &member("bobby", Some("Bob B"))).as_deref(),
            Some("bob@heig-vd.ch")
        );
        assert_eq!(
            author_email(log, &member("ALICE", None)).as_deref(),
            Some("alice@x.ch")
        );
        assert_eq!(author_email(log, &member("carol", None)), None);
    }

    #[test]
    fn parses_porcelain_status() {
        let out = [
            "# branch.oid 5f14f16",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 a b src/my file.c",
            "2 R. N... 100644 100644 100644 a b R100 new name.c",
            "old name.c",
            "u UU N... 100644 100644 100644 100644 a b c both.c",
            "? notes.txt",
            "? .idea/",
            "? src/.DS_Store",
            "? empty/",
            "? work/",
            "1 .M N... 100644 100644 100644 a b .vscode/settings.json",
            "! build/",
            "",
        ]
        .join("\0");
        let entries: Vec<&str> = status_entries(&out).iter().map(|e| e.path).collect();
        assert_eq!(
            entries,
            [
                "src/my file.c",
                "new name.c",
                "both.c",
                "notes.txt",
                ".idea/",
                "src/.DS_Store",
                "empty/",
                "work/",
                ".vscode/settings.json",
            ]
        );
        let state = parse_status(&out, |entry| {
            noise::root(entry).is_some() || entry.ends_with('/') && entry != "work/"
        });
        assert_eq!(state.head.as_deref(), Some("5f14f16"));
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!((state.ahead, state.behind), (Some(2), Some(1)));
        // Tracked files count even in a noise folder; untracked noise and the folder holding
        // only noise don't.
        assert_eq!(state.changed_files, 6);
        assert_eq!(state.tracked_changes, 4);
        assert_eq!(
            state.changed_paths,
            [
                "src/my file.c",
                "new name.c",
                "both.c",
                "notes.txt",
                "work/",
                ".vscode/settings.json",
            ]
        );
        let many: String = (0..25).map(|i| format!("? f{i}.c\0")).collect();
        let state = parse_status(&many, |_| false);
        assert_eq!(state.changed_files, 25);
        assert_eq!(state.changed_paths.len(), MAX_CHANGED_PATHS);
        assert_eq!(state.changed_paths[19], "f19.c");
        assert_eq!(state.tracked_changes, 0);

        let fresh = parse_status("# branch.oid (initial)\0# branch.head (detached)\0", |_| {
            false
        });
        assert_eq!(fresh, LocalState::default());
    }

    /// A real clone: untracked editor files and a new folder holding only a `.DS_Store`
    /// don't count; a new folder with real work does, once.
    #[tokio::test]
    async fn local_state_skips_editor_files() {
        let root = std::env::temp_dir().join(format!("c50tb-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        std::fs::create_dir_all(root.join(".idea")).unwrap();
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join(".idea").join("workspace.xml"), "x").unwrap();
        std::fs::create_dir_all(root.join("empty").join("__pycache__")).unwrap();
        std::fs::write(root.join("empty").join(".DS_Store"), "x").unwrap();
        std::fs::write(root.join("empty").join("__pycache__").join("m.pyc"), "x").unwrap();
        assert_eq!(local_state(&root).await.unwrap().changed_files, 0);

        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::write(root.join("work").join("a.c"), "x").unwrap();
        std::fs::write(root.join("work").join("b.c"), "x").unwrap();
        std::fs::write(root.join("hello.c~"), "x").unwrap();
        assert_eq!(local_state(&root).await.unwrap().changed_files, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A pushed Maven lab opened in IntelliJ, with no `.gitignore`: `.idea/` and the module's
    /// `target/` don't count, a tracked file under `target/` does. A bare `out/` (VS Code's
    /// PlantUML export) is work.
    #[tokio::test]
    async fn local_state_skips_build_output() {
        let root = std::env::temp_dir().join(format!("c50tb-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        let module = root.join("poo-labo-4");
        let classes = module.join("target").join("classes");
        std::fs::create_dir_all(module.join("src")).unwrap();
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(module.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(module.join("src").join("Main.java"), "class Main {}").unwrap();
        git(&["add", "--all"]);
        git(&["commit", "-q", "-m", "lab"]);
        std::fs::create_dir_all(root.join(".idea")).unwrap();
        std::fs::write(root.join(".idea").join("workspace.xml"), "x").unwrap();
        std::fs::create_dir_all(&classes).unwrap();
        std::fs::write(classes.join("Main.class"), "x").unwrap();
        std::fs::create_dir_all(module.join("target").join("generated-sources")).unwrap();
        let state = local_state(&root).await.unwrap();
        assert_eq!(state.changed_files, 0, "{:?}", state.changed_paths);

        // Tracked is never noise, and the untracked files beside it still are.
        std::fs::write(module.join("target").join("notes.txt"), "a").unwrap();
        git(&["add", "poo-labo-4/target/notes.txt"]);
        git(&["commit", "-q", "-m", "notes"]);
        std::fs::write(module.join("target").join("notes.txt"), "b").unwrap();
        let state = local_state(&root).await.unwrap();
        assert_eq!(state.changed_paths, ["poo-labo-4/target/notes.txt"]);

        let png = root.join("out").join("rapport").join("poudlard");
        std::fs::create_dir_all(&png).unwrap();
        std::fs::write(png.join("poudlard.png"), "png").unwrap();
        let _ = std::fs::remove_dir_all(root.join(".idea"));
        let state = local_state(&root).await.unwrap();
        assert_eq!(state.changed_paths, ["poo-labo-4/target/notes.txt", "out/"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
