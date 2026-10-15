<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, withDialog, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";
  import { session } from "$lib/session.svelte";

  let {
    status,
    onchange,
    checkUpdates = false,
  }: {
    status: ToolStatus;
    onchange: (s: ToolStatus) => void;
    /** Ask GitHub whether a newer gh-student exists (Settings only). */
    checkUpdates?: boolean;
  } = $props();

  let installing = $state(false);
  let upgrading = $state(false);
  /** The newer version an update would install. */
  let latest = $state<string | null>(null);
  const busy = $derived(installing || upgrading);
  // gh-student is a gh extension: it can't be installed before gh. Follows the shared
  // statuses, so the button enables as soon as gh is installed or found by Check again.
  const needsGh = $derived(
    status.tool === "gh-student" &&
      !session.tools.some((tool) => tool.tool === "gh" && tool.installed),
  );

  // git and gh are updated by the OS package manager; only gh-student is checked. Runs
  // again whenever the status is re-read (Check again, after an update).
  $effect(() => {
    latest = null;
    if (!checkUpdates || !status.installed || status.tool !== "gh-student") return;
    let current = true;
    api
      .toolUpdate(status.tool)
      .then((version) => {
        if (current) latest = version;
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  });

  async function upgrade() {
    upgrading = true;
    try {
      onchange(await api.upgradeTool(status.tool));
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

<div class="tool">
  <div class="row">
    <span class="dot" class:ok={status.installed}></span>
    <span class="name">{t(`setup.tool.${status.tool}`)}</span>
    {#if status.installed}
      <span class="meta" class:update={latest}
        >{latest
          ? t("setup.newVersion", { current: status.version ?? "", latest })
          : status.version}</span
      >
      {#if latest}
        <button class="primary" disabled={busy} onclick={upgrade}>
          {upgrading ? t("setup.updating") : t("setup.update")}
        </button>
      {/if}
    {:else}
      {#if needsGh}
        <span class="meta">{t("setup.needsGh")}</span>
      {/if}
      <button class="primary" disabled={busy || needsGh} onclick={install}>
        {installing ? t("setup.installing") : t("setup.install")}
      </button>
    {/if}
  </div>
  {#if busy}
    <!-- gh and the package managers report no progress, so the bar only shows it's working. -->
    <div
      class="progress"
      role="progressbar"
      aria-label={t(upgrading ? "setup.updating" : "setup.installing")}
    >
      <div></div>
    </div>
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

  .meta.update {
    color: var(--accent);
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

  .progress {
    height: 2px;
    margin: -2px 12px 6px;
    border-radius: 1px;
    overflow: hidden;
    background: var(--bg-control);
  }

  .progress > div {
    width: 30%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
</style>
