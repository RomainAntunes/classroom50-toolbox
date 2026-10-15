// Pulling a project from the list. Rust only fast-forwards the default branch and refuses
// anything that would need a merge, so the refusals are explained here.
import { message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog, type Project } from "$lib/api";
import { t } from "$lib/i18n";

const REASONS = ["localChanges", "otherBranch", "diverged"];

/** Pulls and reports a refusal or failure. Returns true when the clone was updated. */
export async function pullProject(project: Project, name: string): Promise<boolean> {
  try {
    await api.pullProject(project.id);
    return true;
  } catch (err) {
    const kind = errorKind(err) ?? "";
    const text = REASONS.includes(kind)
      ? t(`pull.${kind}`, { branch: project.remote?.defaultBranch ?? "main" })
      : t("pull.failed", { error: errorMessage(err) });
    await withDialog(() => message(text, { title: name, kind: "warning" }));
    return false;
  }
}
