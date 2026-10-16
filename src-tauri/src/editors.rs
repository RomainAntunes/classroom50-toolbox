//! Finds where the student keeps code without asking: the recent-project lists of the
//! editors students use (JetBrains IDEs, VS Code and its forks, Zed, Sublime Text, Eclipse,
//! NetBeans, Visual Studio), plus the editors' default project folders. Each recent folder
//! remembers which editor opened it. The editors' files are only read, never written.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::{platform, process};

/// An editor the app recognizes, by a stable id and the name shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Editor {
    pub id: &'static str,
    pub name: &'static str,
}

const fn editor(id: &'static str, name: &'static str) -> Editor {
    Editor { id, name }
}

pub const IDEA: Editor = editor("idea", "IntelliJ IDEA");
pub const IDEA_CE: Editor = editor("idea-ce", "IntelliJ IDEA CE");
pub const CLION: Editor = editor("clion", "CLion");
pub const PYCHARM: Editor = editor("pycharm", "PyCharm");
pub const PYCHARM_CE: Editor = editor("pycharm-ce", "PyCharm CE");
pub const RUSTROVER: Editor = editor("rustrover", "RustRover");
pub const WEBSTORM: Editor = editor("webstorm", "WebStorm");
pub const GOLAND: Editor = editor("goland", "GoLand");
pub const PHPSTORM: Editor = editor("phpstorm", "PhpStorm");
pub const RIDER: Editor = editor("rider", "Rider");
pub const RUBYMINE: Editor = editor("rubymine", "RubyMine");
pub const DATAGRIP: Editor = editor("datagrip", "DataGrip");
pub const DATASPELL: Editor = editor("dataspell", "DataSpell");
pub const ANDROID_STUDIO: Editor = editor("android-studio", "Android Studio");
pub const VSCODE: Editor = editor("vscode", "Visual Studio Code");
pub const VSCODE_INSIDERS: Editor = editor("vscode-insiders", "VS Code Insiders");
pub const VSCODIUM: Editor = editor("vscodium", "VSCodium");
pub const CODE_OSS: Editor = editor("code-oss", "Code - OSS");
pub const CURSOR: Editor = editor("cursor", "Cursor");
pub const WINDSURF: Editor = editor("windsurf", "Windsurf");
pub const KIRO: Editor = editor("kiro", "Kiro");
pub const TRAE: Editor = editor("trae", "Trae");
pub const POSITRON: Editor = editor("positron", "Positron");
pub const VOID: Editor = editor("void", "Void");
pub const ANTIGRAVITY: Editor = editor("antigravity", "Antigravity");
pub const ZED: Editor = editor("zed", "Zed");
pub const SUBLIME: Editor = editor("sublime", "Sublime Text");
pub const ECLIPSE: Editor = editor("eclipse", "Eclipse");
pub const NETBEANS: Editor = editor("netbeans", "NetBeans");
pub const VISUAL_STUDIO: Editor = editor("visual-studio", "Visual Studio");

/// JetBrains config folders are `<prefix><version>`; longer prefixes first, so `PyCharmCE`
/// isn't read as `PyCharm`.
const JETBRAINS_PRODUCTS: &[(&str, Editor)] = &[
    ("IntelliJIdea", IDEA),
    ("IdeaIC", IDEA_CE),
    ("CLion", CLION),
    ("PyCharmCE", PYCHARM_CE),
    ("PyCharm", PYCHARM),
    ("RustRover", RUSTROVER),
    ("WebStorm", WEBSTORM),
    ("GoLand", GOLAND),
    ("PhpStorm", PHPSTORM),
    ("Rider", RIDER),
    ("RubyMine", RUBYMINE),
    ("DataGrip", DATAGRIP),
    ("DataSpell", DATASPELL),
    ("AndroidStudio", ANDROID_STUDIO),
];

/// Default "new project" folders in the home dir, with the editor that makes them.
pub(crate) const DEFAULT_ROOTS: &[(&str, Option<Editor>)] = &[
    ("IdeaProjects", Some(IDEA)),
    ("CLionProjects", Some(CLION)),
    ("PycharmProjects", Some(PYCHARM)),
    ("RustroverProjects", Some(RUSTROVER)),
    ("WebstormProjects", Some(WEBSTORM)),
    ("GolandProjects", Some(GOLAND)),
    ("PhpstormProjects", Some(PHPSTORM)),
    ("RiderProjects", Some(RIDER)),
    ("RubymineProjects", Some(RUBYMINE)),
    ("DataspellProjects", Some(DATASPELL)),
    ("AndroidStudioProjects", Some(ANDROID_STUDIO)),
    ("eclipse-workspace", Some(ECLIPSE)),
    ("NetBeansProjects", Some(NETBEANS)),
    ("source/repos", Some(VISUAL_STUDIO)),
    // Common hand-made code folders, never the home dir itself.
    ("Developer", None),
    ("Projects", None),
    ("projects", None),
    ("dev", None),
    ("code", None),
    ("src", None),
    ("repos", None),
    ("git", None),
];

/// Config folder names of VS Code and the forks sharing its storage format.
const VSCODE_PRODUCTS: &[(&str, Editor)] = &[
    ("Code", VSCODE),
    ("Code - Insiders", VSCODE_INSIDERS),
    ("VSCodium", VSCODIUM),
    ("Code - OSS", CODE_OSS),
    ("Cursor", CURSOR),
    ("Windsurf", WINDSURF),
    ("Kiro", KIRO),
    ("Trae", TRAE),
    ("Positron", POSITRON),
    ("Void", VOID),
    ("Antigravity", ANTIGRAVITY),
];

/// Every editor the app knows, for the settings' editor choice.
pub const ALL: &[Editor] = &[
    IDEA,
    IDEA_CE,
    CLION,
    PYCHARM,
    PYCHARM_CE,
    RUSTROVER,
    WEBSTORM,
    GOLAND,
    PHPSTORM,
    RIDER,
    RUBYMINE,
    DATAGRIP,
    DATASPELL,
    ANDROID_STUDIO,
    VSCODE,
    VSCODE_INSIDERS,
    VSCODIUM,
    CODE_OSS,
    CURSOR,
    WINDSURF,
    KIRO,
    TRAE,
    POSITRON,
    VOID,
    ANTIGRAVITY,
    ZED,
    SUBLIME,
    ECLIPSE,
    NETBEANS,
    VISUAL_STUDIO,
];

pub fn by_id(id: &str) -> Option<Editor> {
    ALL.iter().copied().find(|editor| editor.id == id)
}

/// The editors installed here that can open a folder. Blocking (file checks).
pub fn installed() -> Vec<Editor> {
    ALL.iter()
        .copied()
        .filter(|editor| platform::editor_launcher(editor.id).is_some())
        .collect()
}

/// Opens `dir` in the editor `id`.
pub fn open(id: &str, dir: &Path) -> Result<()> {
    let editor = by_id(id).ok_or_else(|| Error::Unsupported(format!("unknown editor `{id}`")))?;
    let launcher = platform::editor_launcher(editor.id).ok_or_else(|| Error::NotInstalled {
        program: editor.name.to_owned(),
    })?;
    let mut args: Vec<std::ffi::OsString> = launcher.args.into_iter().map(Into::into).collect();
    args.push(dir.into());
    process::spawn_detached(&launcher.program, args, Some(dir))
}

/// The editor that last worked on `path`, judged from the folders editors opened: the
/// folder itself, then a folder inside it, then the nearest folder holding it (a course
/// folder, or a default project folder like `~/CLionProjects`).
pub fn editor_for(path: &Path, recents: &[Recent]) -> Option<Editor> {
    let exact = recents.iter().find(|r| r.path == path);
    let inside = || recents.iter().find(|r| r.path.starts_with(path));
    let holding = || {
        recents
            .iter()
            .filter(|r| path.starts_with(&r.path))
            .max_by_key(|r| r.path.components().count())
    };
    [exact, inside(), holding()]
        .into_iter()
        .flatten()
        .find_map(|recent| recent.editor)
}

/// Recent folders kept per editor install; older entries are rarely class work.
const MAX_RECENT_PER_FILE: usize = 30;

/// A folder an editor opened (or holds as its default project folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub path: PathBuf,
    pub editor: Option<Editor>,
}

#[derive(Debug, Default)]
pub struct EditorDirs {
    /// Folders opened recently in an editor, newest first per editor.
    pub recent: Vec<Recent>,
    /// Default project folders that exist, like `~/IdeaProjects`.
    pub roots: Vec<Recent>,
    /// The recent-project files read, watched so a newly opened project shows up.
    pub files: Vec<PathBuf>,
}

/// Blocking: call from `spawn_blocking`.
pub fn find() -> EditorDirs {
    let mut dirs = platform::editor_config_dirs();
    for dir in platform::editor_data_dirs() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    find_in(platform::home_dir(), &dirs)
}

/// `find` over given home and editor dirs, so tests can point it at a fake home.
fn find_in(home: Option<PathBuf>, dirs: &[PathBuf]) -> EditorDirs {
    let mut found = EditorDirs::default();
    let mut seen = BTreeSet::new();

    for dir in dirs {
        for (file, editor) in jetbrains_files(dir) {
            let Ok(xml) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = jetbrains_recent(&xml, home.as_deref());
            add_recent(&mut found, &mut seen, file, editor, paths);
        }
        for (product, editor) in VSCODE_PRODUCTS {
            let storage = dir.join(product).join("User/globalStorage");
            // `storage.json` lists the open windows, `state.vscdb` (SQLite) the recent list.
            for name in ["storage.json", "state.vscdb"] {
                let file = storage.join(name);
                let Ok(bytes) = std::fs::read(&file) else {
                    continue;
                };
                let paths = file_uris(&String::from_utf8_lossy(&bytes));
                add_recent(&mut found, &mut seen, file, Some(*editor), paths);
            }
        }
        if let Some(home) = &home {
            for file in zed_files(dir) {
                let Ok(bytes) = std::fs::read(&file) else {
                    continue;
                };
                let paths = paths_under(&String::from_utf8_lossy(&bytes), home);
                add_recent(&mut found, &mut seen, file, Some(ZED), paths);
            }
        }
        for file in sublime_files(dir) {
            let Ok(json) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = sublime_recent(&json);
            add_recent(&mut found, &mut seen, file, Some(SUBLIME), paths);
        }
        for file in netbeans_files(dir) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = netbeans_recent(&text);
            add_recent(&mut found, &mut seen, file, Some(NETBEANS), paths);
        }
        for file in visual_studio_files(dir) {
            let Ok(xml) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = visual_studio_recent(&xml);
            add_recent(&mut found, &mut seen, file, Some(VISUAL_STUDIO), paths);
        }
    }

    if let Some(home) = &home {
        for file in eclipse_files(home) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = eclipse_workspaces(&text);
            add_recent(&mut found, &mut seen, file, Some(ECLIPSE), paths);
        }
        // Case-insensitive file systems see `Projects` and `projects` as one folder.
        let mut roots_seen = BTreeSet::new();
        found.roots = DEFAULT_ROOTS
            .iter()
            .map(|(name, editor)| Recent {
                path: home.join(name),
                editor: *editor,
            })
            .filter(|root| root.path.is_dir())
            .filter(|root| {
                let key = std::fs::canonicalize(&root.path).unwrap_or_else(|_| root.path.clone());
                roots_seen.insert(key)
            })
            .collect();
    }
    found
}

/// Keeps the first `MAX_RECENT_PER_FILE` folders of `paths`. A file entry (a Rider `.sln`,
/// a `.code-workspace`) stands for the folder holding it.
fn add_recent(
    found: &mut EditorDirs,
    seen: &mut BTreeSet<PathBuf>,
    file: PathBuf,
    editor: Option<Editor>,
    paths: Vec<PathBuf>,
) {
    let dirs = paths
        .into_iter()
        .filter_map(|path| {
            if path.is_dir() {
                Some(path)
            } else if path.is_file() {
                path.parent().map(Path::to_path_buf)
            } else {
                None
            }
        })
        .take(MAX_RECENT_PER_FILE);
    found.recent.extend(
        dirs.filter(|path| seen.insert(path.clone()))
            .map(|path| Recent { path, editor }),
    );
    found.files.push(file);
}

/// `<config>/JetBrains/<Product><version>/options/recentProjects.xml` (`recentSolutions.xml`
/// for Rider), newest version first, and the same under `Google/AndroidStudio*`.
fn jetbrains_files(config_dir: &Path) -> Vec<(PathBuf, Option<Editor>)> {
    let mut files = Vec::new();
    for vendor in ["JetBrains", "Google"] {
        let Ok(entries) = std::fs::read_dir(config_dir.join(vendor)) else {
            continue;
        };
        let mut products: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        products.sort_by(|a, b| b.cmp(a));
        for dir in products {
            let editor = dir
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(jetbrains_product);
            for name in ["recentProjects.xml", "recentSolutions.xml"] {
                let file = dir.join("options").join(name);
                if file.is_file() {
                    files.push((file, editor));
                }
            }
        }
    }
    files
}

/// The product of a config folder name like `IntelliJIdea2026.2`.
fn jetbrains_product(folder: &str) -> Option<Editor> {
    JETBRAINS_PRODUCTS.iter().find_map(|(prefix, editor)| {
        let version = folder.strip_prefix(prefix)?;
        version
            .starts_with(|c: char| c.is_ascii_digit())
            .then_some(*editor)
    })
}

/// The `<entry key="...">` paths of `RecentProjectsManager`, in file order.
fn jetbrains_recent(xml: &str, home: Option<&Path>) -> Vec<PathBuf> {
    const KEY: &str = "<entry key=\"";
    let mut paths = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(KEY) {
        rest = &rest[start + KEY.len()..];
        let Some(end) = rest.find('"') else { break };
        let raw = unescape_xml(&rest[..end]);
        rest = &rest[end..];
        let path = match (raw.strip_prefix("$USER_HOME$"), home) {
            (Some(tail), Some(home)) => format!("{}{tail}", home.display()),
            (Some(_), None) => continue,
            (None, _) => raw,
        };
        // Other macros (`$APPLICATION_CONFIG_DIR$/light-edit`) aren't projects.
        if !path.contains('$') {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

/// Zed's SQLite database per release channel: `<data>/Zed/db/0-stable/db.sqlite`, and its
/// write-ahead log, which holds the newest writes until Zed checkpoints it.
fn zed_files(data_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for name in ["Zed", "zed"] {
        let Ok(entries) = std::fs::read_dir(data_dir.join(name).join("db")) else {
            continue;
        };
        for channel in entries.flatten().map(|entry| entry.path()) {
            for file in ["db.sqlite", "db.sqlite-wal"] {
                // `Zed` and `zed` are one folder on a case-insensitive disk.
                let Ok(file) = std::fs::canonicalize(channel.join(file)) else {
                    continue;
                };
                if file.is_file() && !files.contains(&file) {
                    files.push(file);
                }
            }
        }
    }
    files
}

/// Folders under `home` named in raw text, such as the bytes of a SQLite file where Zed
/// stores workspace paths as plain text separated by newlines. SQLite puts the next column
/// right after a value, so a path that doesn't exist is shortened, within its last
/// component, until it does: `/home/a/lab10` (`lab1` then a `0` column) is `/home/a/lab1`.
fn paths_under(text: &str, home: &Path) -> Vec<PathBuf> {
    let prefix = format!("{}{}", home.display(), std::path::MAIN_SEPARATOR);
    let mut paths = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&prefix) {
        rest = &rest[start..];
        let end = rest
            .find(|c: char| c.is_control() || c == '"' || c == char::REPLACEMENT_CHARACTER)
            .unwrap_or(rest.len());
        let raw = &rest[..end];
        rest = &rest[end..];
        let last = raw.rfind(std::path::MAIN_SEPARATOR).unwrap_or(0);
        let mut candidate = raw;
        while candidate.len() > last + 1 {
            if Path::new(candidate).is_dir() {
                paths.push(PathBuf::from(candidate));
                break;
            }
            let mut cut = candidate.len() - 1;
            while !candidate.is_char_boundary(cut) {
                cut -= 1;
            }
            candidate = &candidate[..cut];
        }
    }
    paths
}

/// `Session.sublime_session` of Sublime Text 4 and 3, in `<config>/<name>/Local`.
fn sublime_files(config_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for name in [
        "Sublime Text",
        "sublime-text",
        "Sublime Text 3",
        "sublime-text-3",
    ] {
        let local = config_dir.join(name).join("Local");
        for file in [
            "Session.sublime_session",
            "Auto Save Session.sublime_session",
        ] {
            let file = local.join(file);
            if file.is_file() && !files.contains(&file) {
                files.push(file);
            }
        }
    }
    files
}

/// The `folder_history` of a Sublime Text session, newest first.
fn sublime_recent(json: &str) -> Vec<PathBuf> {
    let Ok(session) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    session["folder_history"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(sublime_path_to_native)
        .collect()
}

/// Sublime writes Windows paths as `/C/Users/x`.
fn sublime_path_to_native(path: &str) -> PathBuf {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && (bytes.len() == 2 || bytes[2] == b'/');
    if cfg!(windows) && drive {
        return PathBuf::from(format!("{}:{}", &path[1..2], path[2..].replace('/', "\\")));
    }
    PathBuf::from(path)
}

/// `<data>/NetBeans/<version>/config/Preferences/org/netbeans/modules/projectui.properties`.
fn netbeans_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir.join("NetBeans")) else {
        return Vec::new();
    };
    let mut versions: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    versions.sort_by(|a, b| b.cmp(a));
    versions
        .into_iter()
        .map(|version| version.join("config/Preferences/org/netbeans/modules/projectui.properties"))
        .filter(|file| file.is_file())
        .collect()
}

/// The `recentProjectsURL.<n>=file\:/path/` entries, in order.
fn netbeans_recent(properties: &str) -> Vec<PathBuf> {
    let mut entries: Vec<(usize, PathBuf)> = properties
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let index = key
                .trim()
                .strip_prefix("recentProjectsURL.")?
                .parse()
                .ok()?;
            let url = unescape_properties(value.trim());
            let path = url.strip_prefix("file:")?.trim_start_matches("//");
            let path = percent_decode(path)?;
            Some((index, uri_path_to_native(&path)))
        })
        .collect();
    entries.sort_by_key(|(index, _)| *index);
    entries.into_iter().map(|(_, path)| path).collect()
}

/// Eclipse's recent workspaces, in `~/.eclipse/<install>/configuration/.settings`.
fn eclipse_files(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join(".eclipse")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join("configuration/.settings/org.eclipse.ui.ide.prefs")
        })
        .filter(|file| file.is_file())
        .collect()
}

/// `RECENT_WORKSPACES=<path>\n<path>`, newest first. A workspace holds the projects.
fn eclipse_workspaces(prefs: &str) -> Vec<PathBuf> {
    prefs
        .lines()
        .filter_map(|line| line.strip_prefix("RECENT_WORKSPACES="))
        .flat_map(|value| {
            unescape_properties(value)
                .lines()
                .filter(|path| !path.trim().is_empty())
                .map(|path| PathBuf::from(path.trim()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `<LocalAppData>/Microsoft/VisualStudio/<version>/ApplicationPrivateSettings.xml`.
fn visual_studio_files(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(data_dir.join("Microsoft").join("VisualStudio")) else {
        return Vec::new();
    };
    let mut versions: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    versions.sort_by(|a, b| b.cmp(a));
    versions
        .into_iter()
        .map(|version| version.join("ApplicationPrivateSettings.xml"))
        .filter(|file| file.is_file())
        .collect()
}

/// The `"FullPath"` values of the start window's recent list (`CodeContainers.Offline`),
/// JSON stored as XML text: solutions, projects and opened folders.
fn visual_studio_recent(xml: &str) -> Vec<PathBuf> {
    const KEY: &str = "\"FullPath\":\"";
    let json = unescape_xml(xml);
    let mut paths = Vec::new();
    let mut rest = json.as_str();
    while let Some(start) = rest.find(KEY) {
        rest = &rest[start + KEY.len()..];
        let mut value = String::new();
        let mut chars = rest.char_indices();
        let mut end = rest.len();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    end = i;
                    break;
                }
                '\\' => {
                    if let Some((_, escaped)) = chars.next() {
                        value.push(escaped);
                    }
                }
                c => value.push(c),
            }
        }
        rest = &rest[end..];
        if !value.is_empty() {
            paths.push(PathBuf::from(value));
        }
    }
    paths
}

/// Java `.properties` escapes: `\:`, `\=`, `\\` and `\n`.
fn unescape_properties(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn unescape_xml(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Every local `file:///` URI in `text`, decoded to a path, in order of appearance. Works
/// on the raw bytes of VS Code's SQLite file too, since the JSON values are stored as text.
fn file_uris(text: &str) -> Vec<PathBuf> {
    const SCHEME: &str = "file://";
    let mut paths = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(SCHEME) {
        rest = &rest[start + SCHEME.len()..];
        let end = rest
            .find(|c: char| c == '"' || c == '\\' || c.is_control() || c.is_whitespace())
            .unwrap_or(rest.len());
        let encoded = &rest[..end];
        rest = &rest[end..];
        // `file://host/share` (UNC) and remote URIs are skipped.
        if !encoded.starts_with('/') {
            continue;
        }
        if let Some(path) = percent_decode(encoded) {
            paths.push(uri_path_to_native(&path));
        }
    }
    paths
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `/c:/Users/x` is `c:\Users\x` on Windows; elsewhere the path is used as is.
fn uri_path_to_native(path: &str) -> PathBuf {
    let bytes = path.as_bytes();
    if cfg!(windows) && bytes.len() >= 3 && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() {
        return PathBuf::from(path[1..].replace('/', "\\"));
    }
    PathBuf::from(path.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_jetbrains_recent_projects() {
        let xml = r#"<application><component name="RecentProjectsManager">
            <option name="additionalInfo"><map>
              <entry key="$APPLICATION_CONFIG_DIR$/light-edit"><value /></entry>
              <entry key="$USER_HOME$/IdeaProjects/26-poo-c-lab01-group-3"><value /></entry>
              <entry key="/opt/code/R&amp;D"><value /></entry>
            </map></option></component></application>"#;
        assert_eq!(
            jetbrains_recent(xml, Some(Path::new("/Users/alice"))),
            vec![
                PathBuf::from("/Users/alice/IdeaProjects/26-poo-c-lab01-group-3"),
                PathBuf::from("/opt/code/R&D"),
            ]
        );
    }

    #[test]
    fn reads_vscode_file_uris() {
        let json = r#"{"folders":[{"folderUri":"file:///Users/alice/Visual%20Studio/bdr-a-lab1-group-1"},
            {"folderUri":"vscode-vfs://github/o/r"},{"uri":"file://server/share/x"},
            {"folderUri":"file:///Users/alice/code/"}]}"#;
        assert_eq!(
            file_uris(json),
            vec![
                PathBuf::from("/Users/alice/Visual Studio/bdr-a-lab1-group-1"),
                PathBuf::from("/Users/alice/code"),
            ]
        );
    }

    #[test]
    fn names_jetbrains_products_by_config_folder() {
        assert_eq!(jetbrains_product("IntelliJIdea2026.2"), Some(IDEA));
        assert_eq!(jetbrains_product("PyCharmCE2025.1"), Some(PYCHARM_CE));
        assert_eq!(jetbrains_product("PyCharm2026.1"), Some(PYCHARM));
        assert_eq!(
            jetbrains_product("AndroidStudio2025.1"),
            Some(ANDROID_STUDIO)
        );
        assert_eq!(jetbrains_product("consentOptions"), None);
        assert_eq!(jetbrains_product("CLionNova"), None);
    }

    #[test]
    fn reads_zed_paths_from_sqlite_bytes() {
        let home = std::env::temp_dir().join(format!("c50-zed-{}", std::process::id()));
        let lab = home.join("lab1");
        std::fs::create_dir_all(&lab).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        // A `paths` value right before a `paths_order` value of `0`, then a second row
        // whose folder is gone.
        let bytes = format!(
            "\x01\x07{}{sep}lab10\x00\x13{}{sep}gone\x02",
            home.display(),
            home.display()
        );
        assert_eq!(paths_under(&bytes, &home), vec![lab]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn reads_sublime_folder_history() {
        let json = r#"{"folder_history": ["/Users/alice/bdr/lab1", "/Users/alice/poo"],
            "windows": []}"#;
        assert_eq!(
            sublime_recent(json),
            vec![
                PathBuf::from("/Users/alice/bdr/lab1"),
                PathBuf::from("/Users/alice/poo"),
            ]
        );
        assert!(sublime_recent("not json").is_empty());
    }

    #[test]
    fn reads_netbeans_recent_projects() {
        let properties = "openProjectsURLs.0=file\\:/x/\n\
            recentProjectsURL.1=file\\:/Users/alice/NetBeansProjects/Lab%202/\n\
            recentProjectsURL.0=file\\:/Users/alice/NetBeansProjects/Lab1/\n";
        assert_eq!(
            netbeans_recent(properties),
            vec![
                PathBuf::from("/Users/alice/NetBeansProjects/Lab1"),
                PathBuf::from("/Users/alice/NetBeansProjects/Lab 2"),
            ]
        );
    }

    #[test]
    fn reads_eclipse_recent_workspaces() {
        let prefs = "MAX_RECENT_WORKSPACES=10\n\
            RECENT_WORKSPACES=/Users/alice/eclipse-workspace\\n/Users/alice/poo\n";
        assert_eq!(
            eclipse_workspaces(prefs),
            vec![
                PathBuf::from("/Users/alice/eclipse-workspace"),
                PathBuf::from("/Users/alice/poo"),
            ]
        );
    }

    #[test]
    fn reads_visual_studio_recent_list() {
        let xml = r#"<collection name="CodeContainers.Offline"><value name="value">[{&quot;Key&quot;:&quot;C:\\Users\\alice\\source\\repos\\Lab1\\Lab1.sln&quot;,&quot;Value&quot;:{&quot;LocalProperties&quot;:{&quot;FullPath&quot;:&quot;C:\\Users\\alice\\source\\repos\\Lab1\\Lab1.sln&quot;,&quot;Type&quot;:0}}}]</value></collection>"#;
        assert_eq!(
            visual_studio_recent(xml),
            vec![PathBuf::from(r"C:\Users\alice\source\repos\Lab1\Lab1.sln")]
        );
    }

    /// A fake home with one project per editor kind, read end to end.
    #[test]
    fn finds_recent_projects_of_every_editor_kind() {
        let home = std::env::temp_dir().join(format!("c50-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let config = home.join("config");
        let project = |name: &str| {
            let dir = home.join("code").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        // As the editors write them: `/C:/Users/x` in URIs, `/C/Users/x` for Sublime.
        let uri_path = |dir: &Path| {
            let path = dir.display().to_string().replace('\\', "/");
            if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            }
        };
        let sublime_path = |dir: &Path| uri_path(dir).replacen(':', "", 1);
        let write = |path: PathBuf, text: String| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let idea = project("idea-lab");
        let rider = project("rider-lab");
        std::fs::write(rider.join("Lab.sln"), "").unwrap();
        let vscode = project("vscode-lab");
        let zed = project("zed-lab");
        let sublime = project("sublime-lab");
        let netbeans = project("netbeans-lab");
        std::fs::create_dir_all(home.join("IdeaProjects")).unwrap();
        std::fs::create_dir_all(home.join("Developer")).unwrap();

        write(
            config.join("JetBrains/IntelliJIdea2026.2/options/recentProjects.xml"),
            r#"<entry key="$USER_HOME$/code/idea-lab">"#.to_owned(),
        );
        write(
            config.join("JetBrains/Rider2026.1/options/recentSolutions.xml"),
            format!(r#"<entry key="{}">"#, rider.join("Lab.sln").display()),
        );
        write(
            config.join("Cursor/User/globalStorage/storage.json"),
            format!(r#"{{"folderUri":"file://{}"}}"#, uri_path(&vscode)),
        );
        write(
            config.join("Zed/db/0-stable/db.sqlite"),
            format!("\u{1}{}0\u{0}", zed.display()),
        );
        write(
            config.join("Sublime Text/Local/Session.sublime_session"),
            format!(r#"{{"folder_history": ["{}"]}}"#, sublime_path(&sublime)),
        );
        write(
            config.join("NetBeans/24/config/Preferences/org/netbeans/modules/projectui.properties"),
            format!("recentProjectsURL.0=file\\:{}/\n", uri_path(&netbeans)),
        );

        let found = find_in(Some(home.clone()), &[config]);
        let editor_of = |dir: &Path| {
            found
                .recent
                .iter()
                .find(|r| r.path == dir)
                .map(|r| r.editor.map(|e| e.id))
        };
        assert_eq!(editor_of(&idea), Some(Some("idea")));
        assert_eq!(editor_of(&rider), Some(Some("rider")));
        assert_eq!(editor_of(&vscode), Some(Some("cursor")));
        assert_eq!(editor_of(&zed), Some(Some("zed")));
        assert_eq!(editor_of(&sublime), Some(Some("sublime")));
        assert_eq!(editor_of(&netbeans), Some(Some("netbeans")));
        let roots: Vec<_> = found
            .roots
            .iter()
            .map(|r| {
                (
                    r.path.strip_prefix(&home).unwrap().to_owned(),
                    r.editor.map(|e| e.id),
                )
            })
            .collect();
        assert_eq!(
            roots,
            vec![
                (PathBuf::from("IdeaProjects"), Some("idea")),
                (PathBuf::from("Developer"), None),
                (PathBuf::from("code"), None),
            ]
        );
        // Every file read is watched, so a newly opened project shows up.
        assert_eq!(found.files.len(), 6);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn reads_several_zed_paths_from_one_row() {
        let home = std::env::temp_dir().join(format!("c50-zed-rows-{}", std::process::id()));
        let a = home.join("cours-é");
        let b = home.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let text = format!("\u{2}{}\n{}\u{3}0,1", a.display(), b.display());
        assert_eq!(paths_under(&text, &home), vec![a, b]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn picks_the_editor_closest_to_a_clone() {
        let recent = |path: &str, editor| Recent {
            path: PathBuf::from(path),
            editor: Some(editor),
        };
        let recents = vec![
            recent("/home/a/IdeaProjects", IDEA),
            recent("/home/a/IdeaProjects/poo", CURSOR),
            recent("/home/a/IdeaProjects/poo/lab2/src", ZED),
            recent("/home/a/IdeaProjects/poo/lab1", VSCODE),
        ];
        let pick = |path: &str| editor_for(Path::new(path), &recents);
        assert_eq!(pick("/home/a/IdeaProjects/poo/lab1"), Some(VSCODE));
        assert_eq!(pick("/home/a/IdeaProjects/poo/lab2"), Some(ZED));
        assert_eq!(pick("/home/a/IdeaProjects/poo/lab3"), Some(CURSOR));
        assert_eq!(pick("/home/a/IdeaProjects/bdr"), Some(IDEA));
        assert_eq!(pick("/home/a/code/x"), None);
    }

    #[test]
    fn rejects_broken_percent_escapes() {
        assert_eq!(percent_decode("/a%2"), None);
        assert_eq!(percent_decode("/a%zz"), None);
        assert_eq!(percent_decode("/caf%C3%A9").as_deref(), Some("/café"));
    }
}
