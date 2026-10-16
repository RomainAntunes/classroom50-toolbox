<script lang="ts">
  import { message } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, errorMessage, withDialog, type UpdateStatus } from "$lib/api";
  import { formatMegabytes, t } from "$lib/i18n";

  const RELEASES_URL = "https://github.com/RomainAntunes/classroom50-toolbox/releases/tag/v";

  let {
    status,
    compact = false,
  }: {
    status: UpdateStatus;
    /** Settings > About: no "Later" (the main page asks), a "What's new" link instead. */
    compact?: boolean;
  } = $props();

  const update = $derived(status.state);
  // Determinate from the bytes once the server said how many; else it only shows it works.
  const fraction = $derived(
    update.phase === "downloading" && update.total
      ? Math.min(1, update.downloaded / update.total)
      : null,
  );
  const size = $derived(
    update.phase !== "downloading"
      ? ""
      : update.total
        ? t("update.size", {
            downloaded: formatMegabytes(update.downloaded),
            total: formatMegabytes(update.total),
          })
        : formatMegabytes(update.downloaded),
  );
  const hint = $derived(
    status.simulated
      ? t("update.simulatedHint")
      : status.asksPassword
        ? t("update.passwordHint")
        : status.installOnQuit
          ? t(status.deferred || compact ? "update.deferredHint" : "update.readyHint")
          : t("update.restartHint"),
  );

  async function restart() {
    try {
      await api.installUpdate();
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }
</script>

{#if update.phase === "downloading"}
  <div class="banner" class:compact>
    <div class="line">
      <span class="title">{t("update.downloading", { version: update.version })}</span>
      <span class="meta">{size}</span>
    </div>
    <div
      class="bar"
      class:known={fraction !== null}
      role="progressbar"
      aria-label={t("update.downloadingLabel")}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
      aria-valuetext={size}
    >
      <div style:width={fraction === null ? undefined : `${fraction * 100}%`}></div>
    </div>
  </div>
{:else if update.phase === "ready"}
  <div class="banner" class:compact>
    <div class="text">
      <span class="title">{t("update.ready", { version: update.version })}</span>
      <span class="meta">{hint}</span>
    </div>
    <div class="actions">
      {#if compact}
        <button class="link" onclick={() => openUrl(`${RELEASES_URL}${update.version}`)}
          >{t("update.whatsNew")}</button
        >
      {:else if !status.deferred}
        <button onclick={() => api.deferUpdate()}>{t("update.later")}</button>
      {/if}
      <button class="primary" disabled={status.installing} onclick={restart}
        >{t(status.installing ? "update.installing" : "update.restart")}</button
      >
    </div>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-card);
    background: var(--bg-card);
    border: 1px solid var(--border);
  }

  /* Inside a Settings section, which draws the card already. */
  .banner.compact {
    padding: 0 12px 10px;
    background: transparent;
    border: none;
    border-radius: 0;
  }

  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-weight: 600;
  }

  .compact .title {
    font-weight: normal;
  }

  .meta {
    font-size: 0.85em;
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space);
  }

  .link {
    margin-inline-end: auto;
    padding: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: 0.9em;
  }

  .link:hover:not(:disabled) {
    background: transparent;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .bar {
    position: relative;
    height: 4px;
    border-radius: 2px;
    overflow: hidden;
    background: var(--bg-control);
  }

  .bar > div {
    position: absolute;
    inset-block: 0;
    inset-inline: 0 auto;
    width: 30%;
    border-radius: 2px;
    background: var(--accent);
    animation: slide 1.2s ease-in-out infinite;
  }

  .bar.known > div {
    animation: none;
    transition: width 0.2s ease-out;
  }

  /* No sliding: a still, faded bar says it is working without moving. */
  @media (prefers-reduced-motion: reduce) {
    .bar:not(.known) > div {
      width: 100%;
      opacity: 0.4;
    }
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  :global(:root[dir="rtl"]) .bar:not(.known) > div {
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
