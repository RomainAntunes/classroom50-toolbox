//! Why autograder tests failed, read from the body of the graded `submit/*` release. The
//! runner (`render_declarative_body` in upstream's `runner.py`) writes a score heading, a
//! `| Test | Result | Score |` table, then a collapsed section with each failed test's
//! output:
//!
//! ````text
//! <details><summary>Failure details</summary>
//!
//! **<test-name>**
//!
//! ```
//! exit 1; comparison=exact
//! --- input ---
//! ...
//! ```
//!
//! </details>
//! ````
//!
//! The fence is longer than any backtick run inside the block, so it is matched exactly.
//! Passing tests that opted into `show-output` get a `Test output` section, skipped here. The
//! runner may append a note listing files removed by `allowed_files`: whatever isn't the
//! heading, the table, the `Status:` line or one of those two sections is kept as `notes`.
//! Those parts are only recognized when the body has the runner's heading. A custom
//! autograder writes its own body, kept whole as `notes` (its tables, `Status:` lines and
//! `<details>` sections too), minus the `<details>`/`<summary>` tags. Every text is stripped
//! of escape sequences and capped, since it ends up in the snapshot.

use serde::{Deserialize, Serialize};

/// The most kept of one test's output. The runner caps each captured stream at 2000
/// characters, and a failed `io` test shows up to four of them.
pub const TEST_CAP: usize = 6 * 1024;
/// The most kept of all the outputs of one grade together.
pub const TOTAL_CAP: usize = 32 * 1024;
/// The most kept of the body's other text.
pub const NOTES_CAP: usize = 4 * 1024;

const SCORE_HEADING: &str = "### classroom50 autograde:";
const FAILURES_SUMMARY: &str = "Failure details";
/// The runner's section for passing tests that opted into `show-output`.
const OUTPUT_SUMMARY: &str = "Test output";

/// The parts of a release body.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Each failed test's output by test name, in the body's order, uncapped.
    pub tests: Vec<(String, String)>,
    /// The rest of the body, uncapped, empty when there is none.
    pub notes: String,
}

/// What was read from the release body, once per tag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    /// The body's text that isn't a test's output (a custom autograder's report, the files
    /// `allowed_files` removed), capped.
    pub notes: Option<String>,
    pub notes_clipped: bool,
}

/// A text kept with whether it was cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipped {
    pub text: String,
    pub clipped: bool,
}

/// Splits a release body into each failed test's output and the rest. The runner's own
/// parts (score heading, table, `Status:` line, its `Failure details` and `Test output`
/// sections) are only recognized in a body that has the runner's heading; a custom
/// autograder's report keeps its tables, `Status:` lines and sections, with just the
/// `<details>`/`<summary>` tags dropped.
pub fn parse(body: &str) -> Parsed {
    let body = clean(body);
    let runner = body
        .lines()
        .any(|line| line.trim().starts_with(SCORE_HEADING));
    let mut parsed = Parsed::default();
    let mut notes: Vec<&str> = Vec::new();
    let mut lines = body.lines().peekable();
    // Inside one of the runner's `<details>` sections: whether it holds failures (else it's
    // skipped).
    let mut section: Option<bool> = None;
    let mut name: Option<String> = None;
    // Inside a code block of the notes: the backticks that open it.
    let mut notes_fence: Option<usize> = None;
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if let Some(open) = notes_fence {
            if fence(trimmed).is_some_and(|f| f.len() >= open) {
                notes_fence = None;
            }
            notes.push(line);
            continue;
        }
        if let Some(open) = section {
            if trimmed == "</details>" {
                section = None;
                name = None;
            } else if let Some(fence) = fence(trimmed) {
                let mut block: Vec<&str> = Vec::new();
                for inner in lines.by_ref() {
                    if inner.trim_end() == fence {
                        break;
                    }
                    block.push(inner);
                }
                if open && let Some(name) = name.take() {
                    parsed
                        .tests
                        .push((name, block.join("\n").trim_end().to_owned()));
                }
            } else if let Some(test) = bold(trimmed) {
                name = Some(test.to_owned());
            }
            continue;
        }
        if runner {
            if let Some(summary) = details_summary(trimmed)
                && (summary == FAILURES_SUMMARY || summary == OUTPUT_SUMMARY)
            {
                section = Some(summary == FAILURES_SUMMARY);
                continue;
            }
            if trimmed.starts_with(SCORE_HEADING)
                || trimmed.starts_with('|')
                || trimmed.starts_with("Status: ")
            {
                continue;
            }
        }
        if let Some(summary) = without_details_tags(trimmed) {
            if !summary.is_empty() {
                notes.push(summary);
            }
            continue;
        }
        notes_fence = opening_fence(trimmed);
        notes.push(line);
    }
    parsed.notes = tidy(&notes);
    parsed
}

/// Caps each test's output and all of them together, in order.
pub fn cap_outputs(tests: Vec<(String, String)>) -> Vec<(String, Clipped)> {
    let mut left = TOTAL_CAP;
    tests
        .into_iter()
        .map(|(name, text)| {
            let kept = clip(&text, TEST_CAP.min(left));
            left = left.saturating_sub(kept.text.len());
            (name, kept)
        })
        .collect()
}

/// `text` cut to at most `limit` bytes on a character boundary, after the last full line
/// when there is one in the second half.
pub fn clip(text: &str, limit: usize) -> Clipped {
    if text.len() <= limit {
        return Clipped {
            text: text.to_owned(),
            clipped: false,
        };
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(newline) = text[..end].rfind('\n').filter(|&i| i >= end / 2) {
        end = newline;
    }
    Clipped {
        text: text[..end].trim_end().to_owned(),
        clipped: true,
    }
}

/// `\r\n` as `\n`, without escape sequences (CSI and two-byte ones) or control characters
/// other than newlines and tabs. The runner never colors the body, a custom autograder may.
fn clean(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                } else {
                    chars.next();
                }
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => out.push('\n'),
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A code fence line: three or more backticks and nothing else.
fn fence(line: &str) -> Option<&str> {
    (line.len() >= 3 && line.bytes().all(|b| b == b'`')).then_some(line)
}

/// The test name of a `**name**` line.
fn bold(line: &str) -> Option<&str> {
    let name = line.strip_prefix("**")?.strip_suffix("**")?;
    (!name.trim().is_empty()).then_some(name)
}

/// The backticks opening a code block (an info string may follow them).
fn opening_fence(line: &str) -> Option<usize> {
    let ticks = line.bytes().take_while(|&b| b == b'`').count();
    (ticks >= 3 && !line[ticks..].contains('`')).then_some(ticks)
}

/// A line made only of `<details>`, `<summary>` and their closing tags: the summary's text
/// (empty when there is none). `None` for any other line.
fn without_details_tags(line: &str) -> Option<&str> {
    let mut rest = line;
    let mut found = false;
    for tag in ["<details open>", "<details>", "<summary>"] {
        if let Some(after) = rest.strip_prefix(tag) {
            rest = after;
            found = true;
        }
    }
    for tag in ["</details>", "</summary>"] {
        if let Some(before) = rest.strip_suffix(tag) {
            rest = before;
            found = true;
        }
    }
    let rest = rest.trim();
    let only_tags = !rest.contains("<details") && !rest.contains("</details>");
    (found && only_tags && !rest.contains("<summary>") && !rest.contains("</summary>"))
        .then_some(rest)
}

/// The summary of a `<details><summary>X</summary>` line.
fn details_summary(line: &str) -> Option<&str> {
    line.strip_prefix("<details><summary>")?
        .strip_suffix("</summary>")
}

/// The leftover lines as plain text: Markdown heading marks dropped, blank runs folded.
fn tidy(lines: &[&str]) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in lines {
        let line = line.trim_end();
        let line = match line.trim_start_matches('#') {
            rest if rest.len() < line.len() && rest.starts_with(' ') => rest.trim_start(),
            _ => line,
        };
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body as `render_declarative_body` writes it (upstream `runner.py`): an `io` test
    /// failing an exact comparison (diff), a `run` test failing its exit code, a timeout,
    /// a passing test with `show-output`, and the `allowed_files` note appended.
    const DECLARATIVE: &str = "### classroom50 autograde: 3/10\n\
\n\
| Test | Result | Score |\n\
|---|---|---|\n\
| compiles | PASS | 3 / 3 |\n\
| prints \\| pipes | FAIL | 0 / 3 |\n\
| exits cleanly | FAIL | 0 / 2 |\n\
| hangs | FAIL | 0 / 2 |\n\
\n\
<details><summary>Failure details</summary>\n\
\n\
**prints | pipes**\n\
\n\
````\n\
exit 0; comparison=exact\n\
--- input ---\n\
3\n\
--- expected\n\
+++ actual stdout\n\
@@ -1 +1 @@\n\
-```9```\n\
+6\n\
````\n\
\n\
**exits cleanly**\n\
\n\
```\n\
exit 1 (wanted 0)\n\
--- output ---\n\
Traceback (most recent call last):\n\
\x20 File \"hello.py\", line 1\n\
```\n\
\n\
**hangs**\n\
\n\
```\n\
timed out after 10s\n\
```\n\
\n\
</details>\n\
\n\
<details><summary>Test output</summary>\n\
\n\
**compiles**\n\
\n\
```\n\
--- output ---\n\
ok\n\
```\n\
\n\
</details>\n\
\n\
Status: classroom50 autograde: 3/10 (1/4 tests passed)\n\
\n\
### Removed 1 file(s) outside the assignment's allowed files\n\
\n\
These files are not in this assignment's `allowed_files` set, so the autograder did not see them:\n\
\n\
- `notes.txt`\n";

    #[test]
    fn splits_a_declarative_body_per_test() {
        let parsed = parse(DECLARATIVE);
        let names: Vec<&str> = parsed.tests.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["prints | pipes", "exits cleanly", "hangs"]);
        assert_eq!(
            parsed.tests[0].1,
            "exit 0; comparison=exact\n--- input ---\n3\n--- expected\n+++ actual stdout\n\
             @@ -1 +1 @@\n-```9```\n+6"
        );
        assert_eq!(
            parsed.tests[1].1,
            "exit 1 (wanted 0)\n--- output ---\nTraceback (most recent call last):\n  \
             File \"hello.py\", line 1"
        );
        assert_eq!(parsed.tests[2].1, "timed out after 10s");
        assert_eq!(
            parsed.notes,
            "Removed 1 file(s) outside the assignment's allowed files\n\n\
             These files are not in this assignment's `allowed_files` set, so the autograder \
             did not see them:\n\n- `notes.txt`"
        );
    }

    #[test]
    fn the_synthesized_body_leaves_nothing() {
        // `render_release_body`: an autograder that wrote no body of its own.
        let body = "### classroom50 autograde: 0/1\n\n| Test | Result | Score |\n|---|---|---|\n\
                    | a | FAIL | 0 / 1 |\n\nStatus: classroom50 autograde: 0/1 (0/1 tests passed)\n";
        assert_eq!(parse(body), Parsed::default());
        let error = "### classroom50 autograde: 0/0\n\n\
                     _classroom50 autograde: grading error: timed out_\n";
        assert_eq!(
            parse(error).notes,
            "_classroom50 autograde: grading error: timed out_"
        );
    }

    #[test]
    fn keeps_a_custom_body_whole() {
        let body = "## Results\r\n\r\n\u{1b}[31mFAIL\u{1b}[0m check50 `hello`\r\n\r\n\r\n\
                    :( prints hello\r\n    expected \"hello\", not \"\"\u{7}\r\n";
        let parsed = parse(body);
        assert!(parsed.tests.is_empty());
        assert_eq!(
            parsed.notes,
            "Results\n\nFAIL check50 `hello`\n\n:( prints hello\n    expected \"hello\", not \"\""
        );
    }

    #[test]
    fn an_unclosed_section_still_parses() {
        let body = "### classroom50 autograde: 0/1\n\
                    <details><summary>Failure details</summary>\n**a**\n```\nboom\n";
        assert_eq!(parse(body).tests, [("a".to_owned(), "boom".to_owned())]);
    }

    /// A custom autograder's report without the runner's heading: its table, `Status:` line
    /// and every `<details>` section (even one named like the runner's) stay in the notes,
    /// tags dropped, and a code block keeps its `</details>` line.
    #[test]
    fn keeps_a_custom_reports_tables_and_sections() {
        let body = "## Report\n\n\
                    | Check | Result |\n|---|---|\n| style | ok |\n\n\
                    Status: 4/5 checks passed\n\n\
                    <details><summary>Failure details</summary>\n\n**style**\n\n```\nline 3\n```\n\n\
                    </details>\n\n\
                    <details>\n<summary>Lint output</summary>\n\n```text\nwarn: x\n</details>\n```\n\n\
                    </details>\n";
        let parsed = parse(body);
        assert!(parsed.tests.is_empty());
        assert_eq!(
            parsed.notes,
            "Report\n\n| Check | Result |\n|---|---|\n| style | ok |\n\n\
             Status: 4/5 checks passed\n\nFailure details\n\n**style**\n\n```\nline 3\n```\n\n\
             Lint output\n\n```text\nwarn: x\n</details>\n```"
        );
    }

    /// With the runner's heading, a section the runner doesn't write is kept like any note.
    #[test]
    fn keeps_unknown_sections_of_a_runner_body() {
        let body = "### classroom50 autograde: 1/2\n\n| Test | Result | Score |\n\n\
                    <details><summary>Hints</summary>\n\nRead the spec.\n\n</details>\n\n\
                    Status: classroom50 autograde: 1/2\n";
        let parsed = parse(body);
        assert_eq!(parsed.notes, "Hints\n\nRead the spec.");
    }

    #[test]
    fn clips_on_a_line_or_char_boundary() {
        assert_eq!(
            clip("short", 10),
            Clipped {
                text: "short".into(),
                clipped: false
            }
        );
        assert_eq!(clip("line one\nline two", 12).text, "line one");
        let accents = "é".repeat(10);
        let cut = clip(&accents, 5);
        assert_eq!(cut.text, "éé");
        assert!(cut.clipped);
    }

    #[test]
    fn caps_the_outputs_together() {
        let big = "x".repeat(TEST_CAP * 2);
        let tests: Vec<(String, String)> = (0..8).map(|i| (i.to_string(), big.clone())).collect();
        let capped = cap_outputs(tests);
        assert!(
            capped
                .iter()
                .all(|(_, c)| c.clipped && c.text.len() <= TEST_CAP)
        );
        let total: usize = capped.iter().map(|(_, c)| c.text.len()).sum();
        assert_eq!(total, TOTAL_CAP);
        assert!(capped[7].1.text.is_empty());
    }
}
