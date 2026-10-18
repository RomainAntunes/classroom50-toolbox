# Turns `cargo llvm-cov --json --summary-only` into Markdown for the CI run's summary: the
# totals, then one row per file, least covered lines first, paths from the crate's root.
def pct: (.percent * 10 | round / 10 | tostring) + "%";
def counts: "\(pct) (\(.covered)/\(.count))";

.data[0]
| "Lines \(.totals.lines | counts), functions \(.totals.functions | counts), regions \(.totals.regions | pct).",
  "",
  "| File | Lines | Functions | Regions |",
  "| --- | ---: | ---: | ---: |",
  (.files
   | sort_by(.summary.lines.percent, .filename)[]
   | "| \(.filename | sub(".*/src-tauri/"; "")) | \(.summary.lines | counts) | \(.summary.functions | pct) | \(.summary.regions | pct) |")
