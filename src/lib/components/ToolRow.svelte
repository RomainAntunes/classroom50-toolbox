<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";

  let { status, onchange }: { status: ToolStatus; onchange: (s: ToolStatus) => void } = $props();

  let installing = $state(false);

  async function install() {
    try {
      const spec = await api.installCommand(status.tool);
      const command = [spec.program, ...spec.args].join(" ");
      if (!(await ask(t("setup.installConfirm", { command }), { kind: "info" }))) return;
      installing = true;
      onchange(await api.installTool(status.tool));
    } catch (err) {
      await message(errorMessage(err), { kind: "error" });
    } finally {
      installing = false;
    }
  }
</script>

<div class="row">
  <span class="dot" class:ok={status.installed}></span>
  <span class="name">{t(`setup.tool.${status.tool}`)}</span>
  {#if status.installed}
    <span class="meta">{status.version}</span>
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
