<script lang="ts">
  import { goto } from "$app/navigation";
  import { message } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, withDialog, type AppInfo } from "#lib/api.js";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { currentLocale, fromNow, t } from "#lib/i18n/index.js";
  import UpdateBanner from "#lib/components/UpdateBanner.svelte";
  import Section from "#lib/components/Section.svelte";
  import { session } from "#lib/session.svelte.js";
  import { showText } from "#lib/prompt.svelte.js";

  let { info }: { info: AppInfo | null } = $props();

  let checking = $state(false);
  let upToDate = $state(false);

  // A newer version downloads at once; the banner below the row shows it (`update-state`).
  async function checkUpdates() {
    checking = true;
    upToDate = false;
    try {
      const status = await api.checkUpdate();
      upToDate = status.state.phase === "idle";
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      checking = false;
    }
  }

  const update = $derived(session.update);
  const updateBusy = $derived(
    checking ||
      update?.state.phase === "checking" ||
      update?.state.phase === "downloading" ||
      update?.state.phase === "ready",
  );
  const updateNote = $derived.by(() => {
    if (!update) return null;
    if (!update.canUpdate) return t("error.noSelfUpdate");
    if (update.state.phase === "checking") return t("settings.about.checking");
    if (update.state.phase === "error") return errorMessage(update.state.error);
    if (update.lastCheck) {
      return t("settings.about.lastCheck", { when: fromNow(new Date(update.lastCheck)) });
    }
    return null;
  });

  // Copy diagnostics: built in Rust with names masked, previewed, then copied.
  let diagnosing = $state(false);
  let diagnosticsCopied = $state(false);
  async function copyDiagnostics() {
    diagnosing = true;
    diagnosticsCopied = false;
    try {
      const report = await api.diagnosticsReport({
        commit: __COMMIT__ || null,
        skin: document.documentElement.dataset.os ?? info?.os ?? "",
        locale: currentLocale(),
      });
      diagnosing = false;
      const copy = await showText({
        title: t("settings.diagnostics.previewTitle"),
        message: t("settings.diagnostics.masked"),
        text: report.text,
        okLabel: t("settings.diagnostics.copy"),
      });
      if (!copy) return;
      await writeText(report.text);
      diagnosticsCopied = true;
      setTimeout(() => (diagnosticsCopied = false), 2500);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      diagnosing = false;
    }
  }
</script>

<Section title={t("settings.about.title")}>
  <div class="row">
    <span>{t("settings.about.guide")}</span>
    <button onclick={() => goto("/welcome")}>{t("settings.about.openGuide")}</button>
  </div>
  <div class="row">
    <span>{t("settings.about.whatsNew")}</span>
    <button onclick={() => goto("/whats-new?from=settings")}
      >{t("settings.about.openWhatsNew")}</button
    >
  </div>
  <div class="row">
    <span class="text">
      <span>{t("settings.about.updates")}</span>
      {#if updateNote}
        <span class="sub" class:issue={update?.state.phase === "error"}>{updateNote}</span>
      {/if}
    </span>
    <button disabled={updateBusy || update?.canUpdate === false} onclick={checkUpdates}>
      {upToDate ? t("settings.about.upToDate") : t("settings.about.checkUpdates")}
    </button>
  </div>
  {#if update}
    <UpdateBanner status={update} compact />
  {/if}
  <div class="row">
    <span class="text">
      <span>{t("settings.diagnostics.label")}</span>
      <span class="sub">{t("settings.diagnostics.hint")}</span>
    </span>
    <button disabled={diagnosing} onclick={copyDiagnostics}>
      {diagnosticsCopied ? t("settings.diagnostics.copied") : t("settings.diagnostics.action")}
    </button>
  </div>
  <div class="row">
    <span class="text">
      <span>{t("settings.quit.label")}</span>
      <span class="sub">{t("settings.quit.hint")}</span>
    </span>
    <button onclick={() => api.quitApp()}>{t("settings.quit.action")}</button>
  </div>
</Section>

<style>
  @import "./row.css";
  @import "./label.css";

  .sub.issue {
    color: var(--warning);
  }
</style>
