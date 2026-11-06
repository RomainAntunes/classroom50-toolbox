<script lang="ts" module>
  const TABS = ["general", "assignments", "tools", "about"] as const;
  type Tab = (typeof TABS)[number];
  /** The tab shown last, shown again while the app runs. */
  let remembered: Tab = "general";
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorKind,
    onEvent,
    PROJECTS_UPDATED_EVENT,
    type AppInfo,
    type NativeCalendar,
    type SimulatedNetwork,
    type SyncDetails,
  } from "#lib/api.js";
  import { currentDirection, t } from "#lib/i18n/index.js";
  import { atLeast } from "#lib/busy.js";
  import { moveIndex } from "#lib/keys.js";
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
  let calendarDenied = $state(false);

  let checkingTools = $state(false);
  async function checkTools() {
    checkingTools = true;
    try {
      session.tools = await atLeast(api.toolStatuses());
      // The rows' newer-version lookups start on this change; they keep the button busy.
      await tick();
    } finally {
      checkingTools = false;
    }
  }

  const current = $derived(settings.current);

  // One tab at a time, every one kept mounted (hidden), so what each loads on mount stays
  // loaded. "SSH settings…" (`/settings#ssh`) opens Tools, where its row is.
  let tab = $state<Tab>(page.url.hash === "#ssh" ? "tools" : remembered);
  let main = $state<HTMLElement | null>(null);
  const TAB_TITLES: Record<Tab, string> = {
    general: "settings.general.title",
    assignments: "settings.tab.assignments",
    tools: "settings.tools.title",
    about: "settings.about.title",
  };

  function show(next: Tab) {
    if (next === tab) return;
    tab = remembered = next;
    main?.scrollTo({ top: 0 });
  }

  /** Arrows, Home and End move between the tabs and show each, as the list's filters do. */
  function tabKey(event: KeyboardEvent) {
    const next = moveIndex(event.key, TABS.indexOf(tab), TABS.length, {
      horizontal: true,
      wrap: true,
      rtl: currentDirection() === "rtl",
    });
    if (next === null) return;
    event.preventDefault();
    show(TABS[next]);
    document.getElementById(`tab-${TABS[next]}`)?.focus();
  }

  onMount(() => {
    if (!session.tools.length) void checkTools();
    // An editor installed since the app started shows up here.
    void loadEditors(true);
    if (import.meta.env.DEV) void api.simulatedNetwork().then((n) => (network = n));
    void Promise.all([api.appInfo(), loadSettings(), api.syncDetails()]).then(
      ([appInfo, loaded, details]) => {
        info = appInfo;
        sync = details;
        // Names the chosen calendar. Access was granted when it was chosen, but may have
        // been turned off since in System Settings: the section then says so.
        if (appInfo.nativeCalendar && loaded.nativeCalendar) {
          void api.nativeCalendars().then(
            (list) => (calendars = list),
            (err: unknown) => (calendarDenied = errorKind(err) === "calendarAccess"),
          );
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
  <div class="tabs" role="tablist" tabindex="-1" aria-label={t("settings.tabs")} onkeydown={tabKey}>
    {#each TABS as name (name)}
      <button
        role="tab"
        id={`tab-${name}`}
        aria-selected={tab === name}
        aria-controls={`panel-${name}`}
        tabindex={tab === name ? 0 : -1}
        class:active={tab === name}
        onclick={() => show(name)}>{t(TAB_TITLES[name])}</button
      >
    {/each}
  </div>

  <main bind:this={main}>
    <div
      class="panel"
      role="tabpanel"
      id="panel-general"
      aria-labelledby="tab-general"
      hidden={tab !== "general"}
    >
      <GeneralSection {current} {info} />
    </div>

    <div
      class="panel"
      role="tabpanel"
      id="panel-assignments"
      aria-labelledby="tab-assignments"
      hidden={tab !== "assignments"}
    >
      <FoldersSection {current} />
      <RemindersSection {current} />
      <CalendarSection {current} {info} bind:calendars bind:denied={calendarDenied} />
    </div>

    <div
      class="panel"
      role="tabpanel"
      id="panel-tools"
      aria-labelledby="tab-tools"
      hidden={tab !== "tools"}
    >
      <ToolsSection checking={checkingTools} oncheck={checkTools} />
      <SyncSection bind:sync />
    </div>

    <div
      class="panel"
      role="tabpanel"
      id="panel-about"
      aria-labelledby="tab-about"
      hidden={tab !== "about"}
    >
      <AboutSection {info} />

      {#if import.meta.env.DEV}
        <DeveloperSection bind:network />
      {/if}

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
    </div>
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

  /* The list's filters' look: a segmented control under the title. */
  .tabs {
    display: flex;
    gap: 2px;
    /* The space under it stays put: scrolled content goes under that edge, not the tabs. */
    margin: 8px 12px;
    padding: 2px;
    border-radius: 7px;
    background: var(--bg-control);
  }

  .tabs button {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 6px;
    border-radius: 5px;
    background: transparent;
    font-size: 0.8em;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tabs button.active {
    background: var(--bg-card);
    color: var(--fg);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.15);
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
  }

  .panel[hidden] {
    display: none;
  }

  /* Every section's title as tall as one with a button (Tools' Check again, Sync's Clear
     cache: 24px, 25px on Linux), so each tab starts at the same height. */
  .panel > :global(section > header) {
    min-height: 25px;
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
    padding-block: 0 14px;
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
