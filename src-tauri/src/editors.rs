//! Finds where the student keeps code without asking: the recent-project lists of JetBrains
//! IDEs and VS Code (and its forks), plus the IDEs' default project folders. The editors'
//! files are only read, never written.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::platform;

/// JetBrains' default "new project" folders, in the home dir.
const JETBRAINS_ROOTS: &[&str] = &[
    "IdeaProjects",
    "CLionProjects",
    "PycharmProjects",
    "RustroverProjects",
    "WebstormProjects",
    "GolandProjects",
    "PhpstormProjects",
    "RiderProjects",
    "DataspellProjects",
    "AndroidStudioProjects",
];

/// Config folder names of VS Code and the forks sharing its storage format.
const VSCODE_PRODUCTS: &[&str] = &["Code", "Code - Insiders", "VSCodium", "Cursor", "Windsurf"];

/// Recent folders kept per editor install; older entries are rarely class work.
const MAX_RECENT_PER_FILE: usize = 30;

#[derive(Debug, Default)]
pub struct EditorDirs {
    /// Folders opened recently in an editor.
    pub recent: Vec<PathBuf>,
    /// Default project folders that exist, like `~/IdeaProjects`.
    pub roots: Vec<PathBuf>,
    /// The recent-project files read, watched so a newly opened project shows up.
    pub files: Vec<PathBuf>,
}

/// Blocking: call from `spawn_blocking`.
pub fn find() -> EditorDirs {
    let home = platform::home_dir();
    let mut found = EditorDirs::default();
    let mut seen = BTreeSet::new();

    for config_dir in platform::editor_config_dirs() {
        for file in jetbrains_files(&config_dir) {
            let Ok(xml) = std::fs::read_to_string(&file) else {
                continue;
            };
            let paths = jetbrains_recent(&xml, home.as_deref());
            add_recent(&mut found, &mut seen, file, paths);
        }
        for product in VSCODE_PRODUCTS {
            let storage = config_dir.join(product).join("User/globalStorage");
            // `storage.json` lists the open windows, `state.vscdb` (SQLite) the recent list.
            for name in ["storage.json", "state.vscdb"] {
                let file = storage.join(name);
                let Ok(bytes) = std::fs::read(&file) else {
                    continue;
                };
                let paths = file_uris(&String::from_utf8_lossy(&bytes));
                add_recent(&mut found, &mut seen, file, paths);
            }
        }
    }

    if let Some(home) = &home {
        found.roots = JETBRAINS_ROOTS
            .iter()
            .map(|name| home.join(name))
            .filter(|dir| dir.is_dir())
            .collect();
    }
    found
}

fn add_recent(
    found: &mut EditorDirs,
    seen: &mut BTreeSet<PathBuf>,
    file: PathBuf,
    paths: Vec<PathBuf>,
) {
    let dirs = paths
        .into_iter()
        .filter(|path| path.is_dir())
        .take(MAX_RECENT_PER_FILE);
    found
        .recent
        .extend(dirs.filter(|path| seen.insert(path.clone())));
    found.files.push(file);
}

/// `<config>/JetBrains/<Product><version>/options/recentProjects.xml`, newest version first,
/// and the same under `Google/AndroidStudio*`.
fn jetbrains_files(config_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for vendor in ["JetBrains", "Google"] {
        let Ok(entries) = std::fs::read_dir(config_dir.join(vendor)) else {
            continue;
        };
        let mut products: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        products.sort_by(|a, b| b.cmp(a));
        files.extend(
            products
                .into_iter()
                .map(|dir| dir.join("options/recentProjects.xml"))
                .filter(|file| file.is_file()),
        );
    }
    files
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
    fn rejects_broken_percent_escapes() {
        assert_eq!(percent_decode("/a%2"), None);
        assert_eq!(percent_decode("/a%zz"), None);
        assert_eq!(percent_decode("/caf%C3%A9").as_deref(), Some("/café"));
    }
}
