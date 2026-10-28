//! Files editors, the OS and language tools drop into a clone, which most assignment repos
//! don't list in a `.gitignore`: the usual global-gitignore entries. git's own ignore rules
//! apply first; this list only filters what git still reports as **untracked**. A tracked
//! file is never noise (a template may ship `.vscode/settings.json`, and changing it is
//! real work), so callers only ask about untracked paths. Build output folders count as
//! noise only next to the file of the tool that writes them (`build_root`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

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
    // Gradle's and Kotlin's caches, pytest's and mypy's: tools write them, never people.
    ".gradle",
    ".kotlin",
    ".pytest_cache",
    ".mypy_cache",
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

/// Build output folders and the files that say which tool made them, in the same parent
/// folder: Maven, Cargo and sbt write `target/`, Gradle `build/`, Eclipse `bin/`. A bare
/// `build/` or `out/` may well be work (a diagram VS Code's PlantUML exports to `out/`).
const BUILD_DIRS: &[(&str, &[&str])] = &[
    ("target", &["pom.xml", "Cargo.toml", "build.sbt"]),
    (
        "build",
        &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
    ),
    ("bin", &[".classpath"]),
];

/// The files of a clone, as the build output rule asks about them: paths relative to its
/// root in git's `/` form, `""` the root itself and a folder ending with `/`.
pub trait Tree {
    /// `path` (a file or a folder, no trailing `/`) exists.
    fn has(&self, path: &str) -> bool;
    /// The folder `dir` holds a file whose name ends with `suffix`.
    fn holds(&self, dir: &str, suffix: &str) -> bool;
}

/// A clone on disk, each answer kept: a commit's listing names every file of a `target/`,
/// and they all ask the same question.
pub struct OnDisk<'a> {
    root: &'a Path,
    seen: Mutex<HashMap<String, bool>>,
}

impl<'a> OnDisk<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            seen: Mutex::default(),
        }
    }

    fn cached(&self, key: String, look: impl FnOnce() -> bool) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        *seen.entry(key).or_insert_with(look)
    }
}

impl Tree for OnDisk<'_> {
    fn has(&self, path: &str) -> bool {
        self.cached(path.to_owned(), || self.root.join(path).exists())
    }

    fn holds(&self, dir: &str, suffix: &str) -> bool {
        // NUL is in no path, so the key never meets one of `has`.
        self.cached(format!("{dir}\0{suffix}"), || {
            std::fs::read_dir(self.root.join(dir)).is_ok_and(|entries| {
                entries.flatten().any(|e| {
                    e.file_type().is_ok_and(|t| !t.is_dir())
                        && e.file_name().to_str().is_some_and(|n| n.ends_with(suffix))
                })
            })
        })
    }
}

/// A list of the clone's files, for tests.
#[cfg(test)]
impl Tree for [&str] {
    fn has(&self, path: &str) -> bool {
        self.iter().any(|f| {
            f.strip_prefix(path)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
    }

    fn holds(&self, dir: &str, suffix: &str) -> bool {
        self.iter().any(|f| {
            f.strip_prefix(dir)
                .is_some_and(|name| !name.contains('/') && name.ends_with(suffix))
        })
    }
}

/// The build output folder of the untracked `path`, with its trailing `/`: the outermost
/// `target/`, `build/` or `bin/` whose parent folder holds its tool's file (`BUILD_DIRS`),
/// or an IntelliJ project's `out/` (a `.iml` or `.idea/` in the parent folder, where
/// IntelliJ writes it). `None` without one.
pub fn build_root<'a>(path: &'a str, tree: &(impl Tree + ?Sized)) -> Option<&'a str> {
    let mut start = 0;
    while let Some(len) = path[start..].find('/') {
        let end = start + len;
        let (parent, name) = (&path[..start], &path[start..end]);
        let built = if name == "out" {
            tree.holds(parent, ".iml") || tree.has(&format!("{parent}.idea"))
        } else {
            BUILD_DIRS
                .iter()
                .find(|(dir, _)| *dir == name)
                .is_some_and(|(_, markers)| {
                    markers.iter().any(|m| tree.has(&format!("{parent}{m}")))
                })
        };
        if built {
            return Some(&path[..=end]);
        }
        start = end + 1;
    }
    None
}

/// The part of the untracked `path` that makes it noise, by name (`root`) or as build
/// output (`build_root`), the outermost when both apply.
pub fn root_in<'a>(path: &'a str, tree: &(impl Tree + ?Sized)) -> Option<&'a str> {
    [root(path), build_root(path, tree)]
        .into_iter()
        .flatten()
        .min_by_key(|r| r.len())
}

/// Whether the untracked `path` is noise (see `root_in`).
pub fn is_noise(path: &str, tree: &(impl Tree + ?Sized)) -> bool {
    root_in(path, tree).is_some()
}

/// The part of the untracked `path` (git's `/`-separated form, a folder git collapsed ends
/// with `/`) that makes it noise: the noise folder with its trailing `/`, or the whole path
/// for a noise file, by name alone. `None` when its name doesn't make it noise.
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
            assert!(root(path).is_some(), "{path}");
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
            assert!(root(path).is_none(), "{path}");
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

    #[test]
    fn spots_build_output_next_to_its_tool() {
        let cases: &[(&[&str], &str, Option<&str>)] = &[
            (&["pom.xml"], "target/", Some("target/")),
            (&["Cargo.toml"], "target/debug/app", Some("target/")),
            (&["build.sbt"], "target/scala-3/x.class", Some("target/")),
            (&["build.gradle"], "build/", Some("build/")),
            (
                &["build.gradle.kts"],
                "build/classes/A.class",
                Some("build/"),
            ),
            (&["settings.gradle"], "build/x", Some("build/")),
            (&["settings.gradle.kts"], "build/x", Some("build/")),
            (&[".classpath"], "bin/A.class", Some("bin/")),
            (&["lab.iml"], "out/production/A.class", Some("out/")),
            (&[".idea/misc.xml"], "out/", Some("out/")),
            // Nested modules: the marker sits next to the module's own folder.
            (
                &["a/b/pom.xml"],
                "a/b/target/classes/X.class",
                Some("a/b/target/"),
            ),
            (&["m/m.iml"], "m/out/A.class", Some("m/out/")),
            (&["m/.idea/misc.xml"], "m/out/", Some("m/out/")),
            // The outermost build folder wins.
            (
                &["pom.xml", "target/pom.xml"],
                "target/target/x",
                Some("target/"),
            ),
            // No marker, or another tool's: work.
            (&[], "target/x", None),
            (&["build.gradle"], "target/x", None),
            (&["pom.xml"], "build/x", None),
            (&[], "build/", None),
            (&[], "bin/x", None),
            (&[], "out/rapport/poudlard/poudlard.png", None),
            (&["notes.iml.txt"], "out/x.png", None),
            // The marker must be in the parent folder, not below or above it.
            (&["a/pom.xml"], "target/x", None),
            (&["pom.xml"], "a/target/x", None),
            (&["a/m.iml"], "out/x", None),
            (&["a/b/m.iml"], "a/out/x", None),
            // `.idea/` at the clone's root doesn't make a nested `out/` IntelliJ's: VS Code's
            // PlantUML export in a folder of an IntelliJ clone is still work.
            (&[".idea/misc.xml"], "rapport/out/x.png", None),
            // A file named like a build folder is not one.
            (&["pom.xml"], "target", None),
            (&["pom.xml"], "src/target.c", None),
        ];
        for (files, path, root) in cases {
            assert_eq!(build_root(path, *files), *root, "{path} with {files:?}");
        }
    }

    #[test]
    fn noise_by_name_or_as_build_output() {
        let tree: &[&str] = &["pom.xml"];
        assert!(is_noise("target/A.class", tree));
        assert!(is_noise(".idea/", tree));
        assert!(is_noise(".gradle/8.10/x.lock", tree));
        assert!(!is_noise("build/A.class", tree));
        assert!(!is_noise("src/A.java", tree));
        // The outermost of both rules.
        assert_eq!(root_in("target/.DS_Store", tree), Some("target/"));
        assert_eq!(
            root_in(
                "node_modules/m/target/x",
                &["node_modules/m/pom.xml", "pom.xml"][..]
            ),
            Some("node_modules/")
        );
        assert_eq!(root_in("src/.DS_Store", tree), Some("src/.DS_Store"));
    }

    #[test]
    fn reads_markers_from_disk() {
        let root = std::env::temp_dir().join(format!("c50tb-markers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("m").join("target")).unwrap();
        std::fs::create_dir_all(root.join("dir.iml")).unwrap();
        std::fs::write(root.join("m").join("pom.xml"), "<project/>").unwrap();
        std::fs::write(root.join("m").join("m.iml"), "").unwrap();
        let tree = OnDisk::new(&root);
        assert!(tree.has("m/pom.xml"));
        assert!(tree.has("m"));
        assert!(!tree.has("pom.xml"));
        assert!(tree.holds("m/", ".iml"));
        // A folder named like a module file doesn't count.
        assert!(!tree.holds("", ".iml"));
        assert_eq!(build_root("m/target/A.class", &tree), Some("m/target/"));
        assert_eq!(build_root("target/A.class", &tree), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
