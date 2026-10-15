<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { message } from "@tauri-apps/plugin-dialog";
  import { exit } from "@tauri-apps/plugin-process";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    errorMessage,
    PROJECTS_UPDATED_EVENT,
    withDialog,
    type AppInfo,
    type LastRun,
    type SyncDetails,
  } from "$lib/api";
  import { formatDate, fromNow, hoursLabel, t } from "$lib/i18n";
  import { sampleNotices, showNotice } from "$lib/notices";
  import { loadSettings, pickWatchedFolder, saveSettings, settings } from "$lib/settings.svelte";
  import { pickCloneFolder } from "$lib/clone";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import { session } from "$lib/session.svelte";

  const REMINDER_CHOICES = [72, 24, 6, 1];
  const REPO_URL = "https://github.com/RomainAntunes/classroom50-toolbox";
  // A `-dirty` build has no commit page of its own; link the commit it started from.
  const commitUrl = __COMMIT__ ? `${REPO_URL}/commit/${__COMMIT__.replace(/-dirty$/, "")}` : null;

  let info = $state<AppInfo | null>(null);
  let checking = $state(false);
  let upToDate = $state(false);
  let rescanning = $state(false);
  let sync = $state<SyncDetails | null>(null);
  let clearing = $state(false);
  // Diagnostics: folded away unless asked for.
  let showSync = $state(false);
  let showDev = $state(false);
  let badgeQueued = $state(false);

  async function previewBadge() {
    badgeQueued = true;
    try {
      await api.previewBadge();
    } finally {
      badgeQueued = false;
    }
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

  async function rescan() {
    rescanning = true;
    try {
      session.snapshot = await api.rescanLocal();
    } finally {
      rescanning = false;
    }
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
      {#snippet actions()}
        <div class="actions">
          <button disabled={rescanning} onclick={rescan}>{t("projects.rescan")}</button>
          <button onclick={pickWatchedFolder}>{t("projects.addFolder")}</button>
        </div>
      {/snippet}
      <Toggle
        label={t("settings.folders.editors")}
        checked={current.scanEditorProjects}
        onchange={(on) => save({ scanEditorProjects: on })}
      />
      <p class="hint">{t("settings.folders.editorsHint")}</p>
      <div class="row clone-into">
        <span>{t("settings.folders.cloneInto")}</span>
        <span class="path" title={current.cloneFolder ?? undefined}
          >{current.cloneFolder ?? t("settings.folders.cloneUnset")}</span
        >
        <button onclick={pickCloneFolder}>{t("settings.folders.change")}</button>
      </div>
      {#each current.watchedFolders as folder (folder)}
        <div class="row">
          <span class="path" title={folder}>{folder}</span>
          <button onclick={() => removeFolder(folder)}>{t("settings.folders.remove")}</button>
        </div>
      {:else}
        {#if !current.scanEditorProjects}
          <p class="hint">{t("settings.folders.hint")}</p>
        {/if}
      {/each}
    </Section>

    <Section title={t("settings.general.title")}>
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
        <span>{t("settings.about.updates")}</span>
        <button disabled={checking} onclick={checkUpdates}>
          {upToDate ? t("settings.about.upToDate") : t("settings.about.checkUpdates")}
        </button>
      </div>
    </Section>

    <button class="quit" onclick={() => exit(0)}>{t("settings.quit")}</button>

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
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: var(--space) 12px 14px;
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

  .actions {
    display: flex;
    gap: 6px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
  }

  .clone-into .path {
    flex: 1;
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
    background: transparent;
    color: inherit;
    font: inherit;
    font-variant-numeric: tabular-nums;
  }

  .link:hover {
    color: var(--accent);
    text-decoration: underline;
  }

  .quit {
    align-self: center;
    color: var(--danger);
    background: transparent;
  }
</style>
