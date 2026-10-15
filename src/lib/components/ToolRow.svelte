<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { api, errorKind, errorMessage, withDialog, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";
  import { session } from "$lib/session.svelte";
  import { toolJobs, watchToolJobs } from "$lib/toolJobs.svelte";

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

  watchToolJobs();

  /** Set while this row's own request runs, before Rust reports the job and after it ends. */
  let starting = $state<"install" | "upgrade" | null>(null);
  /** The install or update Rust is running for this tool, maybe started from another view. */
  const job = $derived(toolJobs.running[status.tool]);
  const kind = $derived(job?.kind ?? starting);
  const installing = $derived(kind === "install");
  const upgrading = $derived(kind === "upgrade");
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

  /** A failure says what went wrong (the command's last lines) and how to do it by hand. */
  async function failed(err: unknown, key: string, command: string | null) {
    if (errorKind(err) === "cancelled") return;
    const tool = t(`setup.tool.${status.tool}`);
    let text = t(key, { tool, error: errorMessage(err) });
    if (command && errorKind(err) !== "busy") text += t("setup.runYourself", { command });
    await withDialog(() => message(text, { kind: "error" }));
  }

  async function upgrade() {
    if (busy) return;
    starting = "upgrade";
    try {
      onchange(await api.upgradeTool(status.tool));
    } catch (err) {
      await failed(err, "setup.updateFailed", "gh extension upgrade gh-student");
    } finally {
      starting = null;
    }
  }

  async function install() {
    if (busy) return;
    let command: string | null = null;
    try {
      const spec = await api.installCommand(status.tool);
      command = [spec.program, ...spec.args].join(" ");
      const shown = command;
      const confirmed = await withDialog(() =>
        ask(t("setup.installConfirm", { command: shown }), { kind: "info" }),
      );
      if (!confirmed) return;
      starting = "install";
      // gh-student depends on gh, so every row gets its fresh status.
      for (const next of await api.installTool(status.tool)) onchange(next);
    } catch (err) {
      await failed(err, "setup.installFailed", command);
    } finally {
      starting = null;
    }
  }

  function cancel() {
    void api.cancelToolJob(status.tool).catch(() => {});
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
    <!-- No percentage is reported, so the bar only shows it's working; the last line says what. -->
    <div
      class="progress"
      role="progressbar"
      aria-label={t(upgrading ? "setup.updating" : "setup.installing")}
    >
      <div></div>
    </div>
    {#if job}
      <div class="output">
        <span class="line" title={job.line ?? job.command}>{job.line ?? job.command}</span>
        <button onclick={cancel}>{t("setup.cancel")}</button>
      </div>
    {/if}
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

  .output {
    display: flex;
    align-items: center;
    gap: var(--space);
    padding: 0 12px 8px;
  }

  .line {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--fg-muted);
    font-size: 0.9em;
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
