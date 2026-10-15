import { t } from "$lib/i18n";

/** At most `max` names, then "and N more". */
export function fileList(files: string[], max = 5): string {
  const shown = files.slice(0, max).join(", ");
  return files.length > max
    ? t("submit.andMore", { files: shown, count: files.length - max })
    : shown;
}
