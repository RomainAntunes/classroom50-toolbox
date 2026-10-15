<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { message, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { exit } from "@tauri-apps/plugin-process";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    errorMessage,
    APPEARANCES,
    PROJECTS_UPDATED_EVENT,
    SIMULATED_NETWORKS,
    withDialog,
    type AppInfo,
    type LastRun,
    type SimulatedNetwork,
    type SyncDetails,
    type ToolStatus,
  } from "$lib/api";
  import { formatDate, fromNow, hoursLabel, t } from "$lib/i18n";
  import { sampleNotices, showNotice } from "$lib/notices";
  import { loadSettings, pickWatchedFolder, saveSettings, settings } from "$lib/settings.svelte";
  import { pickCloneFolder } from "$lib/clone";
  import { chooseDefaultEditor, defaultEditor, editors, loadEditors } from "$lib/editor.svelte";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";
  import { session } from "$lib/session.svelte";
  import { OS_CHOICES, preview, setPreviewOs } from "$lib/theme/preview.svelte";

  const REMINDER_CHOICES = [72, 24, 6, 1];
  const REPO_URL = "https://github.com/RomainAntunes/classroom50-toolbox";
  // A `-dirty` build has no commit page of its own; link the commit it started from.
  const commitUrl = __COMMIT__ ? `${REPO_URL}/commit/${__COMMIT__.replace(/-dirty$/, "")}` : null;

  let info = $state<AppInfo | null>(null);
  let checking = $state(false);
  let upToDate = $state(false);
  let sync = $state<SyncDetails | null>(null);
  let clearing = $state(false);
  // Diagnostics: folded away unless asked for.
  let showSync = $state(false);
  let showDev = $state(false);
  let badgeQueued = $state(false);
  let network = $state<SimulatedNetwork>("normal");

  async function simulateNetwork(next: SimulatedNetwork) {
    await api.simulateNetwork(next);
    network = next;
  }

  async function previewBadge() {
    badgeQueued = true;
    try {
      await api.previewBadge();
    } finally {
      badgeQueued = false;
    }
  }

  let exported = $state<number | null>(null);
  async function exportCalendar() {
    const path = await withDialog(() =>
      saveDialog({
        defaultPath: "classroom50.ics",
        filters: [{ name: t("settings.calendar.fileType"), extensions: ["ics"] }],
      }),
    );
    if (!path) return;
    try {
      exported = await api.exportCalendar(path);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  let checkingTools = $state(false);
  async function checkTools() {
    checkingTools = true;
    try {
      session.tools = await api.toolStatuses();
    } finally {
      checkingTools = false;
    }
  }

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }

  const rate = $derived(sync?.rateLimit ?? sync?.stats.rateLimit ?? null);

  function lastRun(run: LastRun | null): string {
    if (!run) return t("settings.sync.never");
    return t("settings.sync.run", {
      when: fromNow(new Date(run.at)),
      seconds: run.seconds.toFixed(1),
    });
  }

  async function clearCache() {
    clearing = true;
    try {
      session.snapshot = await api.clearCache();
    } finally {
      clearing = false;
      sync = await api.syncDetails();
    }
  }

  const current = $derived(settings.current);
  const editor = $derived(current ? defaultEditor() : null);

  async function save(patch: Parameters<typeof saveSettings>[0]) {
    try {
      await saveSettings(patch);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  function removeFolder(folder: string) {
    save({ watchedFolders: current?.watchedFolders.filter((f) => f !== folder) ?? [] });
  }

  function toggleReminder(hours: number, on: boolean) {
    const rest = current?.notifyHoursBefore.filter((h) => h !== hours) ?? [];
    save({ notifyHoursBefore: on ? [...rest, hours] : rest });
  }

  async function checkUpdates() {
    checking = true;
    upToDate = false;
    try {
      const update = await api.checkUpdate();
      if (update) await api.installUpdate();
      else upToDate = true;
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    if (!session.tools.length) void checkTools();
    // An editor installed since the app started shows up here.
    void loadEditors(true);
    if (import.meta.env.DEV) void api.simulatedNetwork().then((n) => (network = n));
    void Promise.all([api.appInfo(), loadSettings(), api.syncDetails()]).then(
      ([appInfo, , details]) => {
        info = appInfo;
        sync = details;
      },
    );
    const unlisten = listen(PROJECTS_UPDATED_EVENT, async () => {
      sync = await api.syncDetails();
    });
    return () => void unlisten.then((stop) => stop());
  });
</script>

<header class="titlebar">
  <button class="icon" onclick={() => goto("/")} title={t("app.back")} aria-label={t("app.back")}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </button>
  <h1>{t("app.settings")}</h1>
</header>

{#if current}
  <main>
    <Section title={t("settings.folders.title")}>
      <Toggle
        label={t("settings.folders.editors")}
        hint={t("settings.folders.editorsHint")}
        checked={current.scanEditorProjects}
        onchange={(on) => save({ scanEditorProjects: on })}
      />
      <div class="row">
        <span class="text">
          <span>{t("settings.folders.cloneInto")}</span>
          <span class="sub path" title={current.cloneFolder ?? undefined}
            >{current.cloneFolder ?? t("settings.folders.cloneUnset")}</span
          >
        </span>
        <button onclick={pickCloneFolder}>{t("settings.folders.change")}</button>
      </div>
      <div class="row">
        <span class="text">
          <span>{t("settings.folders.watched")}</span>
          <span class="sub">{t("settings.folders.hint")}</span>
        </span>
        <button onclick={pickWatchedFolder}>{t("projects.addFolder")}</button>
      </div>
      {#each current.watchedFolders as folder (folder)}
        <div class="row nested">
          <span class="path" title={folder}>{folder}</span>
          <button onclick={() => removeFolder(folder)}>{t("settings.folders.remove")}</button>
        </div>
      {/each}
    </Section>

    <Section title={t("settings.tools.title")}>
      {#snippet actions()}
        <button disabled={checkingTools} onclick={checkTools}>{t("setup.recheck")}</button>
      {/snippet}
      <p class="hint">{t("settings.tools.hint")}</p>
      {#each session.tools as status (status.tool)}
        <ToolRow {status} onchange={replaceTool} checkUpdates />
      {/each}
    </Section>

    <Section title={t("settings.general.title")}>
      <div class="row">
        <span class="text">
          <span>{t("settings.editor.label")}</span>
          <span class="sub"
            >{t(editors.installed.length ? "settings.editor.hint" : "settings.editor.none")}</span
          >
        </span>
        {#if editor}
          <button onclick={chooseDefaultEditor}>{editor.name}</button>
        {/if}
      </div>
      {#if current.hidden.length || current.hiddenClasses.length}
        <div class="row">
          <span class="text">
            <span>{t("settings.hidden.label")}</span>
            <span class="sub"
              >{t("settings.hidden.hint", {
                count: current.hidden.length,
                classes: current.hiddenClasses.length,
              })}</span
            >
          </span>
          <button onclick={() => save({ hidden: [], hiddenClasses: [] })}
            >{t("settings.hidden.showAll")}</button
          >
        </div>
      {/if}
      <div class="row">
        <span>{t("settings.appearance")}</span>
        <div class="segmented">
          {#each APPEARANCES as choice (choice)}
            <button
              aria-pressed={current.appearance === choice}
              onclick={() => save({ appearance: choice })}
              >{t(`settings.appearance.${choice}`)}</button
            >
          {/each}
        </div>
      </div>
      <Toggle
        label={t("settings.launchAtLogin")}
        checked={current.launchAtLogin}
        onchange={(on) => save({ launchAtLogin: on })}
      />
    </Section>

    <Section title={t("settings.reminders.title")}>
      {#each REMINDER_CHOICES as hours (hours)}
        <Toggle
          label={t("settings.reminders.before", { duration: hoursLabel(hours) })}
          checked={current.notifyHoursBefore.includes(hours)}
          onchange={(on) => toggleReminder(hours, on)}
        />
      {/each}
    </Section>

    <Section title={t("settings.calendar.title")}>
      <div class="row">
        <span class="text">
          <span>{t("settings.calendar.label")}</span>
          <span class="sub"
            >{exported === null
              ? t("settings.calendar.hint")
              : t("settings.calendar.done", { count: exported })}</span
          >
        </span>
        <button onclick={exportCalendar}>{t("settings.calendar.export")}</button>
      </div>
    </Section>

    {#if sync}
      <Section title={t("settings.sync.title")}>
        {#snippet actions()}
          <button disabled={clearing} onclick={clearCache}>{t("settings.sync.clear")}</button>
        {/snippet}
        <button class="disclosure" aria-expanded={showSync} onclick={() => (showSync = !showSync)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
          {t(showSync ? "settings.sync.hide" : "settings.sync.show")}
        </button>
        {#if showSync}
          <dl class="sync">
            <dt>{t("settings.sync.account")}</dt>
            <dd>{sync.login ?? t("settings.sync.none")}</dd>
            <dt>{t("settings.sync.full")}</dt>
            <dd>{lastRun(sync.stats.full)}</dd>
            <dt>{t("settings.sync.status")}</dt>
            <dd>{lastRun(sync.stats.status)}</dd>
            <dt>{t("settings.sync.local")}</dt>
            <dd>{lastRun(sync.stats.local)}</dd>
            {#if rate}
              <dt>{t("settings.sync.budget")}</dt>
              <dd
                title={rate.resetAt
                  ? t("settings.sync.resets", { when: formatDate(new Date(rate.resetAt)) })
                  : undefined}
              >
                {t("settings.sync.budgetValue", { remaining: rate.remaining, limit: rate.limit })}
              </dd>
            {/if}
            <dt>{t("settings.sync.found")}</dt>
            <dd>
              {t("settings.sync.foundValue", {
                repos: sync.stats.reposSeen,
                assignments: sync.assignments,
                clones: sync.clones,
              })}
            </dd>
            <dt>{t("settings.sync.disk")}</dt>
            <dd>
              {t("settings.sync.diskValue", {
                folders: sync.stats.scanRoots,
                watched: sync.stats.watchTargets,
              })}
            </dd>
            {#if sync.issue}
              <dt>{t("settings.sync.issue")}</dt>
              <dd class="issue" title={sync.issue.message}>{sync.issue.message}</dd>
            {/if}
          </dl>
        {/if}
      </Section>
    {/if}

    {#if import.meta.env.DEV}
      <Section title={t("settings.dev.title")}>
        <div class="row">
          <span title={t("settings.dev.skinHint")}>{t("settings.dev.skin")}</span>
          <div class="segmented">
            {#each OS_CHOICES as os (os)}
              <button aria-pressed={preview.os === os} onclick={() => setPreviewOs(os)}
                >{t(`settings.dev.os.${os}`)}</button
              >
            {/each}
          </div>
        </div>
        <div class="row">
          <span title={t("settings.dev.networkHint")}>{t("settings.dev.network")}</span>
          <div class="segmented">
            {#each SIMULATED_NETWORKS as choice (choice)}
              <button aria-pressed={network === choice} onclick={() => simulateNetwork(choice)}
                >{t(`settings.dev.network.${choice}`)}</button
              >
            {/each}
          </div>
        </div>
        <button class="disclosure" aria-expanded={showDev} onclick={() => (showDev = !showDev)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
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
    {/if}

    <Section title={t("settings.about.title")}>
      <div class="row">
        <span>{t("settings.about.guide")}</span>
        <button onclick={() => goto("/welcome")}>{t("settings.about.openGuide")}</button>
      </div>
      <div class="row">
        <span>{t("settings.about.updates")}</span>
        <button disabled={checking} onclick={checkUpdates}>
          {upToDate ? t("settings.about.upToDate") : t("settings.about.checkUpdates")}
        </button>
      </div>
      <div class="row">
        <span class="text">
          <span>{t("settings.quit.label")}</span>
          <span class="sub">{t("settings.quit.hint")}</span>
        </span>
        <button onclick={() => exit(0)}>{t("settings.quit.action")}</button>
      </div>
    </Section>

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
    /* The 10px track is always there (and invisible), so the right margin is 2px plus the
       track and matches the left one whether or not the page scrolls. */
    overflow-y: scroll;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: var(--space) 2px 14px 12px;
  }

  .disclosure {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 8px 12px;
    border-radius: 0;
    background: transparent;
    text-align: left;
    color: var(--fg-muted);
  }

  .disclosure svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s;
  }

  .disclosure[aria-expanded="true"] svg {
    transform: rotate(90deg);
  }

  .sync {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    margin: 0;
    padding: 10px 12px;
    font-size: 0.9em;
  }

  .sync dt {
    color: var(--fg-muted);
  }

  .sync dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sync .issue {
    color: var(--warning);
  }

  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-control);
    background: var(--bg-control);
  }

  .segmented button {
    padding: 2px 8px;
    background: transparent;
    font-size: 0.9em;
  }

  .segmented button[aria-pressed="true"] {
    background: var(--bg-card);
    box-shadow: 0 0 0 1px var(--border);
  }

  .row.nested {
    padding-left: 24px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .sub {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .hint {
    margin: 0;
    padding: 12px;
    color: var(--fg-muted);
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
