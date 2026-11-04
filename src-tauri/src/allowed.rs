//! An assignment's `allowed_files` (`assignments-v1`): ordered `.gitignore` patterns naming
//! the files that belong to the submission, read as a `.gitignore` at the repo's root (last
//! match wins, `!` re-includes, a folder pattern ends with `/`). A path the patterns would
//! ignore isn't part of the submission: the autograde runner deletes it from its checkout
//! before grading and `gh student submit` leaves it out of the snapshot. `["*", "!hello.py"]`
//! keeps `hello.py` alone; no patterns keep everything.
//!
//! Upstream asks `git check-ignore --no-index` in a throwaway repo, so a file under an
//! ignored folder stays ignored whatever re-includes it (git never looks inside an excluded
//! folder): `["*", "!*.py"]` keeps `a.py` but not `src/b.py`, since `*` excludes `src/`.
//! Here the `ignore` crate matches each line the way git does, and `allows` walks the
//! folders top down the same way. Matching is case-sensitive, as on the runner (Linux),
//! which is what grades. The control files (`.classroom50.yaml`, `.github/`, the runner's
//! `result.json` and `release-body.md`) are always kept. Pure: callers list the files.

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Files and folders the runner and gh-student keep whatever the patterns say (upstream's
/// `ALLOWED_FILES_KEEP_EXACT` and `ALLOWED_FILES_KEEP_PREFIXES`, `isControlPath`): only at
/// the root, and a file only by its exact name (`result.json.bak` is subject to the list).
const CONTROL_FILES: &[&str] = &[".classroom50.yaml", "result.json", "release-body.md"];
const CONTROL_DIRS: &[&str] = &[".github", ".git"];

/// Whether `path` (relative to the repo's root, `/`-separated) is a control file.
pub fn is_control(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    CONTROL_FILES.contains(&path)
        || CONTROL_DIRS.iter().any(|dir| {
            path == *dir
                || path
                    .strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// An assignment's allowlist, ready to match paths.
#[derive(Debug, Clone)]
pub struct Allowlist {
    /// `None` when there are no patterns: everything is part of the submission.
    rules: Option<Gitignore>,
}

impl Allowlist {
    /// Builds the matcher from `patterns` in order. A line git would read but the `ignore`
    /// crate refuses (a broken `[` class) is skipped: upstream fails open too, grading the
    /// whole tree when its matcher can't run.
    pub fn new(patterns: &[String]) -> Self {
        if patterns.is_empty() {
            return Self { rules: None };
        }
        let mut builder = GitignoreBuilder::new("");
        for pattern in patterns {
            if let Err(err) = builder.add_line(None, pattern) {
                log::debug!("allowed_files: skipping a pattern ({err})");
            }
        }
        match builder.build() {
            Ok(rules) if !rules.is_empty() => Self { rules: Some(rules) },
            Ok(_) => Self { rules: None },
            Err(err) => {
                log::debug!("allowed_files: can't build the matcher ({err})");
                Self { rules: None }
            }
        }
    }

    /// No patterns: every file is part of the submission.
    pub fn is_empty(&self) -> bool {
        self.rules.is_none()
    }

    /// Whether `path` (relative to the repo's root, `/`-separated; a folder ends with `/`)
    /// is part of the submission. Every folder above it is checked first, top down, as git
    /// does: one the patterns exclude takes everything under it, even a file a later `!`
    /// names.
    pub fn allows(&self, path: &str) -> bool {
        let Some(rules) = &self.rules else {
            return true;
        };
        if is_control(path) {
            return true;
        }
        let is_dir = path.ends_with('/');
        let path = path.trim_end_matches('/');
        let mut end = 0;
        while let Some(slash) = path[end..].find('/') {
            end += slash;
            if rules.matched(&path[..end], true).is_ignore() {
                return false;
            }
            end += 1;
        }
        !rules.matched(path, is_dir).is_ignore()
    }

    /// The paths of `files` that aren't part of the submission, in their order.
    pub fn not_graded<'a>(&self, files: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        if self.is_empty() {
            return Vec::new();
        }
        files
            .into_iter()
            .filter(|file| !self.allows(file))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(patterns: &[&str]) -> Allowlist {
        Allowlist::new(&patterns.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn keeps_only_what_the_schema_example_names() {
        let allowed = list(&["*", "!hello.py"]);
        assert!(allowed.allows("hello.py"));
        assert!(!allowed.allows("scratch.py"));
        assert!(
            !allowed.allows("notes/hello.py"),
            "under an excluded folder"
        );
        assert!(!allowed.allows("build/"));
        assert_eq!(
            allowed.not_graded(["hello.py", "main.c", "sub/foo.txt"]),
            ["main.c", "sub/foo.txt"]
        );
    }

    #[test]
    fn no_patterns_keep_everything() {
        let allowed = list(&[]);
        assert!(allowed.is_empty());
        assert!(allowed.allows("anything/at/all.bin"));
        assert!(allowed.not_graded(["a.py", "b/"]).is_empty());
        // Blank lines and comments only: nothing to match either.
        assert!(list(&["# just a note"]).is_empty());
    }

    #[test]
    fn the_last_match_wins() {
        let allowed = list(&["*.txt", "!keep.txt", "keep.txt"]);
        assert!(!allowed.allows("keep.txt"));
        assert!(allowed.allows("main.c"));
        let allowed = list(&["*.log", "!important.log"]);
        assert!(allowed.allows("important.log"));
        assert!(allowed.allows("logs/important.log"));
        assert!(!allowed.allows("logs/debug.log"));
    }

    #[test]
    fn matches_folders_and_nested_paths_like_git() {
        let allowed = list(&["node_modules/", "*.lock"]);
        assert!(!allowed.allows("node_modules/x.js"));
        assert!(!allowed.allows("web/node_modules/y/z.js"));
        assert!(!allowed.allows("node_modules/"));
        assert!(
            allowed.allows("node_modules"),
            "a file of that name isn't a folder"
        );
        assert!(!allowed.allows("deep/package.lock"));
        assert!(allowed.allows("src/app.py"));

        // A leading or inner slash anchors the pattern at the root.
        let allowed = list(&["/build", "docs/*.pdf"]);
        assert!(!allowed.allows("build/out.o"));
        assert!(allowed.allows("src/build/out.o"));
        assert!(!allowed.allows("docs/a.pdf"));
        assert!(allowed.allows("docs/sub/a.pdf"));
        assert!(allowed.allows("other/docs/a.pdf"));

        // `**` crosses folders.
        let allowed = list(&["**/tmp/**"]);
        assert!(!allowed.allows("a/b/tmp/c.txt"));
        assert!(allowed.allows("a/b/tmp.txt"));

        // An allowlist of folders: the folder itself has to be re-included, then its files.
        let allowed = list(&["/*", "!/src/", "!/README.md"]);
        assert!(allowed.allows("src/main.c"));
        assert!(allowed.allows("src/deep/util.c"));
        assert!(allowed.allows("README.md"));
        assert!(!allowed.allows("scratch.c"));
        assert!(!allowed.allows("notes/a.md"));
    }

    #[test]
    fn keeps_the_control_files() {
        let allowed = list(&["*", "!hello.py"]);
        for path in [
            ".classroom50.yaml",
            ".github/workflows/autograde.yaml",
            ".github/",
            "result.json",
            "release-body.md",
        ] {
            assert!(allowed.allows(path), "{path}");
        }
        assert!(!allowed.allows("result.json.bak"));
        assert!(!allowed.allows(".githubbed/notes.md"));
        assert!(!allowed.allows("sub/.github/x.yaml"));
    }

    #[test]
    fn is_case_sensitive_like_the_runner() {
        let allowed = list(&["*", "!Main.java"]);
        assert!(allowed.allows("Main.java"));
        assert!(!allowed.allows("main.java"));
    }

    /// Upstream's own fixtures (`cli/shared/testdata`), which pin gh-student's and the
    /// runner's matchers to each other: this one has to agree with both.
    #[test]
    fn agrees_with_upstreams_fixtures() {
        #[derive(serde::Deserialize)]
        struct Cases<T> {
            cases: Vec<T>,
        }
        #[derive(serde::Deserialize)]
        struct Matcher {
            name: String,
            patterns: Vec<String>,
            paths: Vec<String>,
            disallowed: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct Control {
            path: String,
            is_control: bool,
        }
        let read = |name: &str| {
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/allowed-files")
                .join(name);
            std::fs::read_to_string(&file).unwrap()
        };
        let matcher: Cases<Matcher> =
            serde_json::from_str(&read("allowed_files_matcher_cases.json")).unwrap();
        assert!(!matcher.cases.is_empty());
        for case in matcher.cases {
            let allowed = Allowlist::new(&case.patterns);
            let got = allowed.not_graded(case.paths.iter().map(String::as_str));
            let mut want = case.disallowed.clone();
            want.sort();
            let mut got_sorted = got.clone();
            got_sorted.sort();
            assert_eq!(got_sorted, want, "{}", case.name);
        }
        let control: Cases<Control> =
            serde_json::from_str(&read("control_path_cases.json")).unwrap();
        for case in control.cases {
            assert_eq!(is_control(&case.path), case.is_control, "{}", case.path);
        }
    }
}
