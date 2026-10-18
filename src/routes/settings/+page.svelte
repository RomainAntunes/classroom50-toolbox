<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    onEvent,
    PROJECTS_UPDATED_EVENT,
    type AppInfo,
    type NativeCalendar,
    type SimulatedNetwork,
    type SyncDetails,
  } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import { loadSettings, settings } from "#lib/settings.svelte.js";
  import { loadEditors } from "#lib/editor.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import FoldersSection from "#lib/components/settings/Folders.svelte";
  import ToolsSection from "#lib/components/settings/Tools.svelte";
  import GeneralSection from "#lib/components/settings/General.svelte";
  import RemindersSection from "#lib/components/settings/Reminders.svelte";
  import CalendarSection from "#lib/components/settings/Calendar.svelte";
  import SyncSection from "#lib/components/settings/Sync.svelte";
  import DeveloperSection from "#lib/components/settings/Developer.svelte";
  import AboutSection from "#lib/components/settings/About.svelte";

  const REPO_URL = "https://github.com/RomainAntunes/classroom50-toolbox";
  // A `-dirty` build has no commit page of its own; link the commit it started from.
  const commitUrl = __COMMIT__ ? `${REPO_URL}/commit/${__COMMIT__.replace(/-dirty$/, "")}` : null;

  let info = $state<AppInfo | null>(null);
  let sync = $state<SyncDetails | null>(null);
  let network = $state<SimulatedNetwork>("normal");
  // Loaded with the page and named by the Calendar section's menu.
  let calendars = $state<NativeCalendar[]>([]);

  let checkingTools = $state(false);
  async function checkTools() {
    checkingTools = true;
    try {
      session.tools = await api.toolStatuses();
    } finally {
      checkingTools = false;
    }
  }

  const current = $derived(settings.current);

  onMount(() => {
    if (!session.tools.length) void checkTools();
    // An editor installed since the app started shows up here.
    void loadEditors(true);
    if (import.meta.env.DEV) void api.simulatedNetwork().then((n) => (network = n));
    void Promise.all([api.appInfo(), loadSettings(), api.syncDetails()]).then(
      ([appInfo, loaded, details]) => {
        info = appInfo;
        sync = details;
        // Names the chosen calendar; access was granted when it was chosen.
        if (appInfo.nativeCalendar && loaded.nativeCalendar) {
          void api.nativeCalendars().then((list) => (calendars = list));
        }
      },
    );
    const unlisten = onEvent(PROJECTS_UPDATED_EVENT, async () => {
      sync = await api.syncDetails();
    });
    return () => void unlisten.then((stop) => stop());
  });
</script>

<header class="titlebar">
  <button class="icon" onclick={() => goto("/")} title={t("app.back")} aria-label={t("app.back")}>
    <svg class="mirror-rtl" viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </button>
  <h1>{t("app.settings")}</h1>
</header>

{#if current}
  <main>
    <FoldersSection {current} />

    <ToolsSection checking={checkingTools} oncheck={checkTools} />

    <GeneralSection {current} {info} />

    <RemindersSection {current} />

    <CalendarSection {current} {info} bind:calendars />

    <SyncSection bind:sync />

    {#if import.meta.env.DEV}
      <DeveloperSection bind:network />
    {/if}

    <AboutSection {info} />

    <footer>
      {#if info}
        <span>{t("settings.about.version", { version: info.version })}</span>
      {/if}
      {#if __COMMIT__ && commitUrl}
        <span aria-hidden="true">·</span>
        <button class="link" title={t("settings.about.commit")} onclick={() => openUrl(commitUrl)}
          >{__COMMIT__}</button
        >
      {/if}
      <span aria-hidden="true">·</span>
      <button class="link" onclick={() => openUrl(REPO_URL)}>{t("settings.about.github")}</button>
    </footer>
  </main>
{/if}

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: var(--space);
    padding: 12px 14px 4px;
  }

  h1 {
    margin: 0;
    font-size: 1.1em;
    font-weight: 600;
  }

  main {
    flex: 1;
    /* The 10px track is always there (and invisible), on the inline-end side (left when
       right to left), so that margin is 2px plus the track and matches the start one
       whether or not the page scrolls. */
    overflow-y: scroll;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding-block: var(--space) 14px;
    padding-inline: 12px 2px;
  }

  footer {
    display: flex;
    justify-content: center;
    gap: 6px;
    font-size: 0.8em;
    color: var(--fg-muted);
  }

  .link {
    padding: 0;
    border-radius: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }

  /* Beats the global `button:hover` background, which drew a box behind the text. */
  .link:hover:not(:disabled) {
    background: transparent;
    color: var(--fg);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
