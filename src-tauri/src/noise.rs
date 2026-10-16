//! Files editors, the OS and language tools drop into a clone, which most assignment repos
//! don't list in a `.gitignore`: the usual global-gitignore entries. git's own ignore rules
//! apply first; this list only filters what git still reports as **untracked**. A tracked
//! file is never noise (a template may ship `.vscode/settings.json`, and changing it is
//! real work), so callers only ask about untracked paths.

/// Folders skipped at any depth.
const DIRS: &[&str] = &[
    ".idea",
    ".vscode",
    ".fleet",
    ".zed",
    ".vs",
    ".history",
    ".metadata",
    ".nova",
    "__MACOSX",
    "__pycache__",
    "node_modules",
];

/// File names skipped anywhere. Windows spells some of them with any case.
const NAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "ehthumbs.db",
    "desktop.ini",
    ".directory",
];

/// File name endings skipped anywhere.
const SUFFIXES: &[&str] = &[".iml", ".swp", ".swo", "~", ".pyc"];

/// The part of the untracked `path` (git's `/`-separated form, a folder git collapsed ends
/// with `/`) that makes it noise: the noise folder with its trailing `/`, or the whole path
/// for a noise file. `None` when it's real work.
pub fn root(path: &str) -> Option<&str> {
    let mut start = 0;
    while let Some(len) = path[start..].find('/') {
        let end = start + len;
        if DIRS.contains(&&path[start..end]) {
            return Some(&path[..=end]);
        }
        start = end + 1;
    }
    let name = &path[start..];
    if name.is_empty() {
        return None;
    }
    let file = NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
        || name.starts_with("._")
        || SUFFIXES.iter().any(|s| name.ends_with(s));
    file.then_some(path)
}

/// Whether the untracked `path` is noise (see `root`).
pub fn is_noise(path: &str) -> bool {
    root(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_editor_and_os_files() {
        for path in [
            ".idea/",
            ".idea/workspace.xml",
            "src/.vscode/settings.json",
            "a/b/c/__pycache__/m.cpython-312.pyc",
            "node_modules/left-pad/index.js",
            "web/node_modules/",
            "__MACOSX/._a.c",
            ".DS_Store",
            "src/.DS_Store",
            "Thumbs.db",
            "pics/thumbs.db",
            "Desktop.ini",
            ".directory",
            "._hello.c",
            "lab1.iml",
            "src/.hello.c.swp",
            ".hello.c.swo",
            "hello.c~",
            "m.pyc",
        ] {
            assert!(is_noise(path), "{path}");
        }
        for path in [
            "hello.c",
            "src/",
            "notes.txt",
            ".gitignore",
            ".classroom50.yaml",
            "idea/x",
            "my.idea/x",
            ".ideas/x",
            "node_modules_backup/x",
            "src/modules.py",
            "_hello.c",
            "a.iml.txt",
            ".DS_Store/x/y.c",
            "",
        ] {
            assert!(!is_noise(path), "{path}");
        }
    }

    #[test]
    fn names_the_folder_or_the_file() {
        assert_eq!(root("a/.idea/workspace.xml"), Some("a/.idea/"));
        assert_eq!(root(".idea/"), Some(".idea/"));
        // The outermost noise folder wins.
        assert_eq!(
            root("node_modules/x/__pycache__/y.pyc"),
            Some("node_modules/")
        );
        assert_eq!(root("src/.DS_Store"), Some("src/.DS_Store"));
        assert_eq!(root("src/main.c"), None);
    }
}
