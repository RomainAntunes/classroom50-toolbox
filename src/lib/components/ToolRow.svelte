<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorKind,
    errorMessage,
    withDialog,
    type CommandError,
    type ToolStatus,
  } from "#lib/api.js";
  import { hasKey, t } from "#lib/i18n/index.js";
  import { announce } from "#lib/announce.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toolJobs, watchToolJobs } from "#lib/toolJobs.svelte.js";

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
    const kind = errorKind(err);
    if (kind === "cancelled") return;
    const tool = t(`setup.tool.${status.tool}`);
    if (kind === "manualInstall") return manual(err as CommandError, tool);
    const error = kind === "notSignedIn" ? t("setup.signInFirst") : errorMessage(err);
    let text = t(key, { tool, error });
    if (command && kind !== "busy" && kind !== "notSignedIn") {
      text += t("setup.runYourself", { command });
    }
    await withDialog(() => message(text, { kind: "error" }));
  }

  /** The app can't install it here (no package manager, no password prompt): say why and
   * offer the download page or the command for a terminal. */
  async function manual(err: CommandError, tool: string) {
    const reasonKey = `setup.manualReason.${err.params?.reason}`;
    const reason = hasKey(reasonKey) ? t(reasonKey) : err.message;
    let text = t("setup.manual", { tool, reason });
    if (err.command) text += t("setup.runYourself", { command: err.command });
    const url = err.url;
    if (!url) {
      await withDialog(() => message(text, { kind: "info" }));
      return;
    }
    const open = await withDialog(() =>
      ask(text, {
        kind: "info",
        okLabel: t("setup.openDownload"),
        cancelLabel: t("setup.notNow"),
      }),
    );
    if (open) await openUrl(url);
  }

  async function upgrade() {
    if (busy) return;
    starting = "upgrade";
    try {
      onchange(await api.upgradeTool(status.tool));
      announce(t("a11y.toolUpdated", { tool: t(`setup.tool.${status.tool}`) }));
    } catch (err) {
      await failed(err, "setup.updateFailed", "gh extension upgrade gh-student");
    } finally {
      starting = null;
    }
  }

  // No confirmation: the click is the user's go-ahead, and the OS asks for a password itself
  // when the package manager needs one.
  async function install() {
    if (busy) return;
    starting = "install";
    try {
      // gh-student depends on gh, so every row gets its fresh status.
      for (const next of await api.installTool(status.tool)) onchange(next);
      announce(t("a11y.toolInstalled", { tool: t(`setup.tool.${status.tool}`) }));
    } catch (err) {
      const plan = await api.installCommand(status.tool).catch(() => null);
      await failed(err, "setup.installFailed", plan?.manual ?? null);
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
    <span class="dot" class:ok={status.installed && !status.outdated} aria-hidden="true"></span>
    <span class="name"
      >{t(`setup.tool.${status.tool}`)}{#if !status.outdated}<span class="sr-only"
          >, {t(status.installed ? "setup.installed" : "setup.notInstalled")}</span
        >{/if}</span
    >
    {#if status.installed}
      <span
        class="meta"
        class:update={latest}
        class:outdated={status.outdated}
        title={status.outdated ? t("setup.outdatedHint") : undefined}
        >{latest
          ? t("setup.newVersion", { current: status.version ?? "", latest })
          : status.outdated
            ? t("setup.outdated", { version: status.version ?? "" })
            : status.version}</span
      >
      {#if upgrading || installing}
        <button onclick={cancel} disabled={!job}>{t("setup.cancel")}</button>
      {:else if status.outdated}
        <!-- gh is installed again, which upgrades it through the same package manager;
             gh-student is upgraded (`gh extension install` refuses an installed one). -->
        <button class="primary" onclick={status.tool === "gh-student" ? upgrade : install}
          >{t("setup.update")}</button
        >
      {:else if latest}
        <button class="primary" onclick={upgrade}>{t("setup.update")}</button>
      {/if}
    {:else if installing}
      <span class="meta">{t("setup.installing")}</span>
      <button onclick={cancel} disabled={!job}>{t("setup.cancel")}</button>
    {:else}
      {#if needsGh}
        <span class="meta">{t("setup.needsGh")}</span>
      {/if}
      <button class="primary" disabled={needsGh} onclick={install}>{t("setup.install")}</button>
    {/if}
  </div>
  {#if busy}
    <!-- A real bar once the package manager prints a known step; until then it only shows
         it's working (gh prints nothing while it installs an extension). -->
    <div
      class="progress"
      class:known={job?.progress != null}
      role="progressbar"
      aria-label={t(upgrading ? "setup.updating" : "setup.installing")}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={job?.progress != null ? Math.round(job.progress * 100) : undefined}
    >
      <div style:width={job?.progress != null ? `${job.progress * 100}%` : undefined}></div>
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
    color: var(--accent-text);
  }

  .meta.outdated {
    color: var(--warning);
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

  @media (prefers-reduced-motion: reduce) {
    .progress:not(.known) > div {
      width: 100%;
      opacity: 0.4;
    }
  }

  .progress.known > div {
    animation: none;
    transition: width 0.4s ease;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  :global(:root[dir="rtl"]) .progress:not(.known) > div {
    animation-name: slide-rtl;
  }

  /* Right to left the bar starts at the right edge and slides leftward. */
  @keyframes slide-rtl {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(-340%);
    }
  }
</style>
