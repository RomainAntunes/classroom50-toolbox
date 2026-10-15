<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, withDialog, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";

  let { status, onchange }: { status: ToolStatus; onchange: (s: ToolStatus) => void } = $props();

  let installing = $state(false);
  let upgrading = $state(false);
  let upToDate = $state(false);
  // git and gh are updated by the OS package manager; gh-student by gh.
  const upgradable = $derived(status.installed && status.tool === "gh-student");

  async function upgrade() {
    upgrading = true;
    upToDate = false;
    try {
      const next = await api.upgradeTool(status.tool);
      upToDate = next.version === status.version;
      onchange(next);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      upgrading = false;
    }
  }

  async function install() {
    try {
      const spec = await api.installCommand(status.tool);
      const command = [spec.program, ...spec.args].join(" ");
      const confirmed = await withDialog(() =>
        ask(t("setup.installConfirm", { command }), { kind: "info" }),
      );
      if (!confirmed) return;
      installing = true;
      onchange(await api.installTool(status.tool));
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      installing = false;
    }
  }
</script>

<div class="row">
  <span class="dot" class:ok={status.installed}></span>
  <span class="name">{t(`setup.tool.${status.tool}`)}</span>
  {#if status.installed}
    <span class="meta">{upToDate ? t("setup.upToDate") : status.version}</span>
    {#if upgradable}
      <button disabled={upgrading} onclick={upgrade}>
        {upgrading ? t("setup.updating") : t("setup.update")}
      </button>
    {/if}
  {:else}
    <button class="primary" disabled={installing} onclick={install}>
      {installing ? t("setup.installing") : t("setup.install")}
    </button>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space);
    padding: 8px 12px;
  }

  .name {
    flex: 1;
  }

  .meta {
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warning);
  }

  .dot.ok {
    background: var(--success);
  }
</style>
