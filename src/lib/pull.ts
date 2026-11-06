// Pulling a project from the list. Rust only fast-forwards the default branch and refuses
// anything that would need a merge, so the refusals are explained here.
import { message } from "@tauri-apps/plugin-dialog";
import { api, errorKind, errorMessage, withDialog, type Project } from "#lib/api.js";
import { t } from "#lib/i18n/index.js";
import { announce } from "#lib/announce.svelte.js";

const REASONS = ["localChanges", "otherBranch", "diverged"];

/** Pulls and reports a refusal or failure. Returns true when the clone was updated. A
 *  clone whose SSH failed is offered HTTPS, then pulled once more (`retried`). */
export async function pullProject(
  project: Project,
  name: string,
  retried = false,
): Promise<boolean> {
  try {
    await api.pullProject(project.id);
    announce(t("a11y.pulled", { name }));
    return true;
  } catch (err) {
    const kind = errorKind(err) ?? "";
    const text = REASONS.includes(kind)
      ? t(`pull.${kind}`, { branch: project.remote?.defaultBranch ?? "main" })
      : t("pull.failed", { error: errorMessage(err) });
    if (kind === "ssh" && !retried && project.local?.origin === "ssh") {
      const { switchToHttps } = await import("#lib/ssh.js");
      return (await switchToHttps(project, name, text)) && pullProject(project, name, true);
    }
    await withDialog(() => message(text, { title: name, kind: "warning" }));
    return false;
  }
}
