<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { message, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    errorMessage,
    APPEARANCES,
    COMMIT_ACTIONS,
    PROJECTS_UPDATED_EVENT,
    SIMULATED_NETWORKS,
    UPDATE_CHANNELS,
    withDialog,
    type AppInfo,
    type LastRun,
    type NativeCalendar,
    type SimulatedNetwork,
    type SyncDetails,
    type ToolStatus,
  } from "$lib/api";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import {
    catalogs,
    currentLocale,
    formatDate,
    formatNumber,
    fromNow,
    hasKey,
    hoursLabel,
    languageName,
    t,
  } from "$lib/i18n";
  import { sampleNotices, showNotice } from "$lib/notices";
  import { loadSettings, pickWatchedFolder, saveSettings, settings } from "$lib/settings.svelte";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { pickCloneFolder } from "$lib/clone";
  import { chooseDefaultEditor, defaultEditor, editors, loadEditors } from "$lib/editor.svelte";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";
  import UpdateBanner from "$lib/components/UpdateBanner.svelte";
  import { popupMenu } from "$lib/keyboard";
  import { session } from "$lib/session.svelte";
  import { showText } from "$lib/prompt.svelte";
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

  let exported = $state<number | null>(null);
  async function exportCalendar(): Promise<boolean> {
    const path = await withDialog(() =>
      saveDialog({
        defaultPath: "classroom50.ics",
        filters: [{ name: t("settings.calendar.fileType"), extensions: ["ics"] }],
      }),
    );
    if (!path) return false;
    try {
      exported = await api.exportCalendar(path);
      // Rust now keeps that file updated (`calendarFile`).
      await loadSettings();
      return true;
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
      return false;
    }
  }

  // No calendar store to write into (Windows, Linux without GNOME's or KDE's): the calendar
  // app imports the file, and importing it again updates the events (stable UIDs).
  async function openInCalendar() {
    if (!settings.current?.calendarFile && !(await exportCalendar())) return;
    try {
      await api.openCalendarFile();
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  // Worded per kind; anything else (a D-Bus call that failed on Linux) adds the details.
  async function calendarError(err: unknown) {
    const key = `calendar.error.${(err as { kind?: string }).kind}`;
    const text = hasKey(key) ? t(key) : `${t("calendar.error.fallback")}\n\n${errorMessage(err)}`;
    await withDialog(() => message(text, { kind: "error" }));
  }

  /** "Classroom 50 (GNOME Calendar)" on Linux, "Work (iCloud)" on macOS. */
  function calendarLabel(calendar: NativeCalendar): string {
    const account = calendar.kind
      ? t(`settings.calendar.store.${calendar.kind}`)
      : calendar.account;
    return account ? `${calendar.title} (${account})` : calendar.title;
  }

  let calendars = $state<NativeCalendar[]>([]);
  let writing = $state(false);
  const chosenCalendar = $derived(
    calendars.find((c) => c.id === settings.current?.nativeCalendar) ?? null,
  );

  async function setNativeCalendar(id: string | null) {
    writing = true;
    try {
      settings.current = await api.setNativeCalendar(id);
    } catch (err) {
      await calendarError(err);
    } finally {
      writing = false;
    }
  }

  async function chooseCalendar(anchor: HTMLElement) {
    try {
      // The first call raises macOS's access prompt, which takes focus: pinned, the popover
      // stays on Settings for the menu that follows.
      calendars = await withDialog(() => api.nativeCalendars());
    } catch (err) {
      await calendarError(err);
      return;
    }
    const current = settings.current?.nativeCalendar ?? null;
    const items: (MenuItem | PredefinedMenuItem)[] = await Promise.all(
      calendars.map((calendar) =>
        MenuItem.new({
          text: calendarLabel(calendar),
          enabled: calendar.id !== current,
          action: () => setNativeCalendar(calendar.id),
        }),
      ),
    );
    if (current) {
      items.push(
        await PredefinedMenuItem.new({ item: "Separator" }),
        await MenuItem.new({
          text: t("settings.calendar.stopSync"),
          action: () => setNativeCalendar(null),
        }),
      );
    }
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
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
      seconds: formatNumber(run.seconds, 1),
    });
  }

  /** "System", then each catalog named in its own language, as a native menu. */
  async function chooseLanguage(anchor: HTMLElement) {
    const chosen = settings.current?.language ?? "system";
    const codes = ["system", ...Object.keys(catalogs)];
    const items = await Promise.all(
      codes.map((code) =>
        CheckMenuItem.new({
          text: code === "system" ? t("settings.language.system") : languageName(code),
          checked: code === chosen,
          action: () => void save({ language: code }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  /** Stable or Beta, as a native menu. */
  async function chooseUpdateChannel(anchor: HTMLElement) {
    const chosen = settings.current?.updateChannel ?? "stable";
    const items = await Promise.all(
      UPDATE_CHANNELS.map((choice) =>
        CheckMenuItem.new({
          text: t(`settings.updates.${choice}`),
          checked: choice === chosen,
          action: () => void save({ updateChannel: choice }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  /** Off, Commit or Commit & push, as a native menu. */
  async function chooseCommitActions(anchor: HTMLElement) {
    const chosen = settings.current?.commitActions ?? "none";
    const items = await Promise.all(
      COMMIT_ACTIONS.map((choice) =>
        CheckMenuItem.new({
          text: t(`settings.commitActions.${choice}`),
          checked: choice === chosen,
          action: () => void save({ commitActions: choice }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
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

  // A newer version downloads at once; the banner below the row shows it (`update-state`).
  async function checkUpdates() {
    checking = true;
    upToDate = false;
    try {
      const status = await api.checkUpdate();
      upToDate = status.state.phase === "idle";
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      checking = false;
    }
  }

  const update = $derived(session.update);
  const updateBusy = $derived(
    checking ||
      update?.state.phase === "checking" ||
      update?.state.phase === "downloading" ||
      update?.state.phase === "ready",
  );
  const updateNote = $derived.by(() => {
    if (!update) return null;
    if (!update.canUpdate) return t("error.noSelfUpdate");
    if (update.state.phase === "checking") return t("settings.about.checking");
    if (update.state.phase === "error") return errorMessage(update.state.error);
    if (update.lastCheck) {
      return t("settings.about.lastCheck", { when: fromNow(new Date(update.lastCheck)) });
    }
    return null;
  });

  // MSI installs stay on stable (pre-releases ship no MSI).
  const channel = $derived(
    update?.hasBeta === false ? "stable" : (current?.updateChannel ?? "stable"),
  );

  // Copy diagnostics: built in Rust with names masked, previewed, then copied.
  let diagnosing = $state(false);
  let diagnosticsCopied = $state(false);
  async function copyDiagnostics() {
    diagnosing = true;
    diagnosticsCopied = false;
    try {
      const report = await api.diagnosticsReport({
        commit: __COMMIT__ || null,
        skin: document.documentElement.dataset.os ?? info?.os ?? "",
        locale: currentLocale(),
      });
      diagnosing = false;
      const copy = await showText({
        title: t("settings.diagnostics.previewTitle"),
        message: t("settings.diagnostics.masked"),
        text: report.text,
        okLabel: t("settings.diagnostics.copy"),
      });
      if (!copy) return;
      await writeText(report.text);
      diagnosticsCopied = true;
      setTimeout(() => (diagnosticsCopied = false), 2500);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      diagnosing = false;
    }
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
        // Names the chosen calendar; access was granted when it was chosen.
        if (appInfo.nativeCalendar && loaded.nativeCalendar) {
          void api.nativeCalendars().then((list) => (calendars = list));
        }
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
          <button
            aria-haspopup="menu"
            aria-label={`${t("settings.editor.label")}: ${editor.name}`}
            onclick={(event) => chooseDefaultEditor(event.currentTarget)}>{editor.name}</button
          >
        {/if}
      </div>
      <div class="row">
        <span class="text">
          <span>{t("settings.commitActions")}</span>
          <span class="sub">{t(`settings.commitActions.${current.commitActions}Hint`)}</span>
        </span>
        <button
          aria-haspopup="menu"
          aria-label={`${t("settings.commitActions")}: ${t(`settings.commitActions.${current.commitActions}`)}`}
          onclick={(event) => chooseCommitActions(event.currentTarget)}
          >{t(`settings.commitActions.${current.commitActions}`)}</button
        >
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
        <span id="appearance-label">{t("settings.appearance")}</span>
        <div class="segmented" role="group" aria-labelledby="appearance-label">
          {#each APPEARANCES as choice (choice)}
            <button
              aria-pressed={current.appearance === choice}
              onclick={() => save({ appearance: choice })}
              >{t(`settings.appearance.${choice}`)}</button
            >
          {/each}
        </div>
      </div>
      <div class="row">
        <span>{t("settings.language")}</span>
        <button
          aria-haspopup="menu"
          aria-label={`${t("settings.language")}: ${
            current.language === "system"
              ? t("settings.language.system")
              : languageName(current.language)
          }`}
          onclick={(event) => chooseLanguage(event.currentTarget)}
          >{current.language === "system"
            ? t("settings.language.system")
            : languageName(current.language)}</button
        >
      </div>
      <Toggle
        label={t("settings.launchAtLogin")}
        checked={current.launchAtLogin}
        onchange={(on) => save({ launchAtLogin: on })}
      />
      <div class="row">
        <span class="text">
          <span>{t("settings.updates.channel")}</span>
          <span class="sub"
            >{update?.hasBeta === false
              ? t("settings.updates.msiHint")
              : t(`settings.updates.${channel}Hint`)}</span
          >
        </span>
        <button
          aria-haspopup="menu"
          disabled={update?.hasBeta === false}
          aria-label={`${t("settings.updates.channel")}: ${t(`settings.updates.${channel}`)}`}
          onclick={(event) => chooseUpdateChannel(event.currentTarget)}
          >{t(`settings.updates.${channel}`)}</button
        >
      </div>
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
      {#if info?.nativeCalendar}
        <div class="row">
          <span class="text">
            <span>{t("settings.calendar.app")}</span>
            <span class="sub"
              >{current.nativeCalendar
                ? t("settings.calendar.syncing", {
                    calendar: chosenCalendar
                      ? calendarLabel(chosenCalendar)
                      : t("settings.calendar.unknown"),
                  })
                : t("settings.calendar.appHint")}</span
            >
          </span>
          <button
            disabled={writing}
            aria-haspopup="menu"
            onclick={(event) => chooseCalendar(event.currentTarget)}
            >{t(
              current.nativeCalendar ? "settings.calendar.change" : "settings.calendar.choose",
            )}</button
          >
        </div>
      {/if}
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
      {#if info && !info.nativeCalendar}
        <div class="row">
          <span class="text">
            <span>{t("settings.calendar.openLabel")}</span>
            <span class="sub">{t("settings.calendar.openHint")}</span>
          </span>
          <button onclick={openInCalendar}>{t("settings.calendar.open")}</button>
        </div>
      {/if}
      {#if current.calendarFile}
        <div class="row">
          <span class="text">
            <span>{t("settings.calendar.kept")}</span>
            <span class="sub path" title={current.calendarFile}>{current.calendarFile}</span>
          </span>
          <button onclick={() => save({ calendarFile: null })}>{t("settings.calendar.stop")}</button
          >
        </div>
      {/if}
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
                {t("settings.sync.budgetValue", { remaining: rate.remaining, count: rate.limit })}
              </dd>
            {/if}
            <dt>{t("settings.sync.found")}</dt>
            <dd>
              {t("settings.sync.foundValue", {
                repos: t("settings.sync.repos", { count: sync.stats.reposSeen }),
                assignments: t("settings.sync.assignments", { count: sync.assignments }),
                clones: t("settings.sync.cloned", { count: sync.clones }),
              })}
            </dd>
            <dt>{t("settings.sync.disk")}</dt>
            <dd>
              {t("settings.sync.diskValue", {
                folders: t("settings.sync.folders", { count: sync.stats.scanRoots }),
                watched: t("settings.sync.watched", { count: sync.stats.watchTargets }),
              })}
            </dd>
            {#if sync.issue}
              <dt>{t("settings.sync.issue")}</dt>
              <dd class="issue" title={errorMessage(sync.issue)}>{errorMessage(sync.issue)}</dd>
            {/if}
          </dl>
        {/if}
      </Section>
    {/if}

    {#if import.meta.env.DEV}
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
        <div class="row">
          <span id="network-label" title={t("settings.dev.networkHint")}
            >{t("settings.dev.network")}</span
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
        <span class="text">
          <span>{t("settings.about.updates")}</span>
          {#if updateNote}
            <span class="sub" class:issue={update?.state.phase === "error"}>{updateNote}</span>
          {/if}
        </span>
        <button disabled={updateBusy || update?.canUpdate === false} onclick={checkUpdates}>
          {upToDate ? t("settings.about.upToDate") : t("settings.about.checkUpdates")}
        </button>
      </div>
      {#if update}
        <UpdateBanner status={update} compact />
      {/if}
      <div class="row">
        <span class="text">
          <span>{t("settings.diagnostics.label")}</span>
          <span class="sub">{t("settings.diagnostics.hint")}</span>
        </span>
        <button disabled={diagnosing} onclick={copyDiagnostics}>
          {diagnosticsCopied ? t("settings.diagnostics.copied") : t("settings.diagnostics.action")}
        </button>
      </div>
      <div class="row">
        <span class="text">
          <span>{t("settings.quit.label")}</span>
          <span class="sub">{t("settings.quit.hint")}</span>
        </span>
        <button onclick={() => api.quitApp()}>{t("settings.quit.action")}</button>
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

  .sync .issue,
  .sub.issue {
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
