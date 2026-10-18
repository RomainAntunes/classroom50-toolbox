import { t } from "#lib/i18n/index.js";

/** At most `max` names, then "and N more". */
export function fileList(files: string[], max = 5): string {
  const shown = files.slice(0, max).join(", ");
  return files.length > max
    ? t("submit.andMore", { files: shown, count: files.length - max })
    : shown;
}

/** The clone's uncommitted paths, one per line, at most `max`, then "and N more" for the rest
 *  (Rust names 20 and counts them all). Just the count when no path is known. */
export function changedLines(
  local: { changedFiles: number; changedPaths?: string[] },
  max = 20,
): string {
  const shown = (local.changedPaths ?? []).slice(0, max);
  if (!shown.length) return String(local.changedFiles);
  const rest = local.changedFiles - shown.length;
  return rest > 0
    ? [...shown, t("details.moreFiles", { count: rest })].join("\n")
    : shown.join("\n");
}
