<script lang="ts">
  import { goto } from "$app/navigation";
  import { api, SIMULATED_NETWORKS, type SimulatedNetwork } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import { sampleNotices, showNotice } from "#lib/notices.js";
  import Section from "#lib/components/Section.svelte";
  import Toggle from "#lib/components/Toggle.svelte";
  import { OS_CHOICES, preview, setPreviewOs, setPreviewRtl } from "#lib/theme/preview.svelte.js";
  import { locale } from "#lib/i18n/locale.svelte.js";

  let { network = $bindable() }: { network: SimulatedNetwork } = $props();

  let showDev = $state(false);
  let badgeQueued = $state(false);

  async function simulateNetwork(next: SimulatedNetwork) {
    await api.simulateNetwork(next);
    network = next;
  }

  // Fires a link through the same handler as the OS: bundled builds only get real ones.
  let devLink = $state("");
  async function openDevLink() {
    await api.simulateDeepLink(devLink);
    devLink = "";
  }

  async function previewBadge() {
    badgeQueued = true;
    try {
      await api.previewBadge();
    } finally {
      badgeQueued = false;
    }
  }
</script>

<Section title={t("settings.dev.title")}>
  <div class="row">
    <span id="skin-label" title={t("settings.dev.skinHint")}>{t("settings.dev.skin")}</span>
    <div class="segmented" role="group" aria-labelledby="skin-label">
      {#each OS_CHOICES as os (os)}
        <button aria-pressed={preview.os === os} onclick={() => setPreviewOs(os)}
          >{t(`settings.dev.os.${os}`)}</button
        >
      {/each}
    </div>
  </div>
  <Toggle
    label={t("settings.dev.rtl")}
    hint={t("settings.dev.rtlHint")}
    checked={locale.rtlPreview}
    onchange={setPreviewRtl}
  />
  <div class="row">
    <span id="network-label" title={t("settings.dev.networkHint")}>{t("settings.dev.network")}</span
    >
    <div class="segmented" role="group" aria-labelledby="network-label">
      {#each SIMULATED_NETWORKS as choice (choice)}
        <button aria-pressed={network === choice} onclick={() => simulateNetwork(choice)}
          >{t(`settings.dev.network.${choice}`)}</button
        >
      {/each}
    </div>
  </div>
  <form
    class="row"
    onsubmit={(event) => {
      event.preventDefault();
      void openDevLink();
    }}
  >
    <input
      class="dev-link"
      type="text"
      placeholder={t("settings.dev.linkPlaceholder")}
      title={t("settings.dev.linkHint")}
      aria-label={t("settings.dev.link")}
      spellcheck="false"
      autocomplete="off"
      bind:value={devLink}
    />
    <button type="submit" disabled={!devLink.trim()}>{t("settings.dev.linkOpen")}</button>
  </form>
  <div class="row">
    <span title={t("settings.dev.updateHint")}>{t("settings.dev.update")}</span>
    <button onclick={() => api.simulateUpdate()}>{t("settings.dev.updateRun")}</button>
  </div>
  <div class="row">
    <span title={t("settings.dev.whatsNewHint")}>{t("settings.dev.whatsNew")}</span>
    <button onclick={() => goto("/whats-new?from=settings&all")}
      >{t("settings.dev.whatsNewShow")}</button
    >
  </div>
  <button class="disclosure" aria-expanded={showDev} onclick={() => (showDev = !showDev)}>
    <svg class="mirror-rtl" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
    {t("settings.dev.notifications")}
  </button>
  {#if showDev}
    <p class="hint">{t("settings.dev.hint")}</p>
    {#each sampleNotices() as notice (notice.kind)}
      <div class="row">
        <span>{t(`settings.dev.kind.${notice.kind}`)}</span>
        <button onclick={() => showNotice(notice)}>{t("settings.dev.send")}</button>
      </div>
    {/each}
    <div class="row">
      <span title={t("settings.dev.badgeHint")}>{t("settings.dev.badge")}</span>
      <button disabled={badgeQueued} onclick={previewBadge}>
        {t(badgeQueued ? "settings.dev.badgeQueued" : "settings.dev.badgeShow")}
      </button>
    </div>
  {/if}
</Section>

<style>
  @import "./row.css";
  @import "./hint.css";
  @import "./segmented.css";
  @import "./disclosure.css";

  .dev-link {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    height: 24px;
    font: inherit;
    font-size: 0.9em;
    color: inherit;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  .dev-link:focus-visible {
    border-color: var(--focus-ring);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }
</style>
