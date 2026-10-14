<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { listen } from "@tauri-apps/api/event";
  import { Menu, MenuItem } from "@tauri-apps/api/menu";
  import {
    api,
    PROJECTS_UPDATED_EVENT,
    SYNC_PROGRESS_EVENT,
    UPDATE_AVAILABLE_EVENT,
    type AuthStatus,
    type Project,
    type Snapshot,
    type SyncProgress,
    type Unaccepted,
    type ToolStatus,
    type UpdateInfo,
    withDialog,
  } from "$lib/api";
  import { fromNow, t } from "$lib/i18n";
  import { session } from "$lib/session.svelte";
  import { loadSettings } from "$lib/settings.svelte";
  import Section from "$lib/components/Section.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";
  import ProjectRow from "$lib/components/ProjectRow.svelte";
  import UnacceptedRow from "$lib/components/UnacceptedRow.svelte";
  import LoginStatus from "$lib/components/LoginStatus.svelte";

  interface ClassGroup {
    key: string;
    title: string;
    owner: string;
    projects: Project[];
    unaccepted: Unaccepted[];
  }

  let now = $state(new Date());
  let loading = $state(false);

  const missingTool = $derived(session.tools.some((tool) => !tool.installed));
  const ghInstalled = $derived(session.tools.some((tool) => tool.tool === "gh" && tool.installed));
  // Signing in is already offered in the footer.
  const issue = $derived(
    session.snapshot?.issue?.kind === "notSignedIn" ? null : session.snapshot?.issue,
  );
  // Background checks stay silent; only syncs the user started show progress.
  const syncing = $derived(
    loading ||
      (session.progress?.running && session.progress.foreground) ||
      session.snapshot?.syncedAt == null,
  );
  // Indeterminate until the sync knows how much work it has.
  const fraction = $derived(
    session.progress?.total ? session.progress.done / session.progress.total : null,
  );
  // A status check also counts: it is the last time GitHub was read.
  const syncedAt = $derived.by(() => {
    const at = session.snapshot?.checkedAt ?? session.snapshot?.syncedAt;
    return at ? new Date(at) : null;
  });
  // `now` ticks every minute, so anything under a minute reads "just now".
  const syncedLabel = $derived(
    syncedAt === null
      ? null
      : now.getTime() - syncedAt.getTime() < 60_000
        ? t("projects.syncedJustNow")
        : t("projects.synced", { when: fromNow(syncedAt, now) }),
  );
  const allProjects = $derived(session.snapshot?.projects ?? []);
  const query = $derived(session.search.trim().toLowerCase());
  const unseen = $derived(new Set(session.unseen));
  const groups = $derived(
    groupByClass(
      allProjects.filter((p) => matches(q(p), query)),
      (session.snapshot?.unaccepted ?? []).filter((u) =>
        matches(
          [
            u.assignment.name,
            u.assignment.slug,
            u.classroom.slug,
            u.classroom.name,
            u.classroom.org,
          ],
          query,
        ),
      ),
    ),
  );

  function q(project: Project): (string | null | undefined)[] {
    return [
      project.assignment?.name,
      project.config.assignment,
      project.config.classroom,
      project.classroom?.name,
      project.owner,
      project.repo,
    ];
  }

  function matches(fields: (string | null | undefined)[], search: string): boolean {
    return !search || fields.some((field) => field?.toLowerCase().includes(search));
  }

  /** Keeps Rust's urgency order: a class appears where its most urgent assignment is. */
  function groupByClass(projects: Project[], unaccepted: Unaccepted[]): ClassGroup[] {
    const groups = new Map<string, ClassGroup>();
    const group = (owner: string, slug: string, title: string) => {
      const key = `${owner}/${slug}`.toLowerCase();
      let found = groups.get(key);
      if (!found) {
        found = { key, title, owner, projects: [], unaccepted: [] };
        groups.set(key, found);
      }
      return found;
    };
    for (const project of projects) {
      const { classroom } = project.config;
      group(project.owner ?? "", classroom, project.classroom?.name ?? classroom).projects.push(
        project,
      );
    }
    for (const item of unaccepted) {
      const { org, slug, name } = item.classroom;
      group(org, slug, name ?? slug).unaccepted.push(item);
    }
    // Two orgs can use the same classroom slug; name the org only then.
    const list = [...groups.values()];
    for (const group of list) {
      const [owner] = group.key.split("/");
      if (owner && list.some((g) => g !== group && g.title === group.title)) {
        group.title = `${group.title} (${group.owner})`;
      }
    }
    return list;
  }

  /** Cheap local state: settings, tools, login and the last synced list. */
  async function load() {
    await loadSettings();
    const [tools, auth, snapshot, progress] = await Promise.all([
      api.toolStatuses(),
      api.authStatus(),
      api.listProjects(),
      api.syncProgress(),
    ]);
    session.tools = tools;
    session.progress = progress;
    session.auth = auth;
    // The startup sync may still be running; its event fills the list.
    if (snapshot.syncedAt || !session.snapshot) session.snapshot = snapshot;
    now = new Date();
  }

  /** The refresh button: everything, including a GitHub sync. */
  async function refresh() {
    loading = true;
    try {
      await loadSettings();
      const [tools, auth, snapshot] = await Promise.all([
        api.toolStatuses(),
        api.authStatus(),
        api.refreshProjects(),
      ]);
      Object.assign(session, { tools, auth, snapshot });
      now = new Date();
    } finally {
      loading = false;
    }
  }

  /** Disk only: picks up clones and commits without a GitHub request. */
  async function rescan() {
    loading = true;
    try {
      session.snapshot = await api.rescanLocal();
      now = new Date();
    } finally {
      loading = false;
    }
  }

  async function refreshMenu(event: MouseEvent) {
    event.preventDefault();
    const items = await Promise.all([
      MenuItem.new({ text: t("app.refresh"), enabled: !loading, action: refresh }),
      MenuItem.new({ text: t("projects.rescan"), enabled: !loading, action: rescan }),
    ]);
    const menu = await Menu.new({ items });
    await withDialog(() => menu.popup());
  }

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }

  async function signedIn(next: AuthStatus) {
    session.auth = next;
    session.snapshot = await api.refreshProjects();
  }

  onMount(() => {
    load();
    const unlisteners = [
      listen<UpdateInfo>(UPDATE_AVAILABLE_EVENT, (event) => {
        session.update = event.payload;
      }),
      listen<SyncProgress>(SYNC_PROGRESS_EVENT, (event) => {
        session.progress = event.payload;
      }),
      listen<Snapshot>(PROJECTS_UPDATED_EVENT, (event) => {
        // `gh auth switch`: the footer follows the account the data now belongs to.
        if (event.payload.login && event.payload.login !== session.auth?.login) {
          void api.authStatus().then((auth) => {
            session.auth = auth;
          });
        }
        session.snapshot = event.payload;
        now = new Date();
      }),
    ];
    // Keeps "due in ..." labels current while the popover stays open.
    const tick = setInterval(() => (now = new Date()), 60_000);
    return () => {
      clearInterval(tick);
      for (const unlisten of unlisteners) void unlisten.then((stop) => stop());
    };
  });
</script>

<header class="titlebar">
  <h1>{t("app.title")}</h1>
  <input
    class="search"
    type="search"
    placeholder={t("projects.search")}
    aria-label={t("projects.search")}
    spellcheck="false"
    autocomplete="off"
    bind:value={session.search}
  />
  <div class="tools">
    <button
      class="icon"
      class:spinning={syncing}
      disabled={loading}
      onclick={refresh}
      oncontextmenu={refreshMenu}
      title={syncedLabel ? `${t("app.refresh")} (${syncedLabel})` : t("app.refresh")}
      aria-label={t("app.refresh")}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
        <path d="M21 3v5h-5" />
      </svg>
    </button>
    <button
      class="icon"
      onclick={() => goto("/settings")}
      title={t("app.settings")}
      aria-label={t("app.settings")}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
        />
        <circle cx="12" cy="12" r="3" />
      </svg>
    </button>
  </div>
  {#if syncing}
    <div
      class="progress"
      class:indeterminate={fraction === null}
      role="progressbar"
      aria-label={t("projects.syncing")}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
    >
      <div style:width={fraction === null ? undefined : `${fraction * 100}%`}></div>
    </div>
  {/if}
</header>

<main>
  {#if session.update}
    <div class="banner">
      <span>{t("update.available", { version: session.update.version })}</span>
      <button class="primary" onclick={() => api.installUpdate()}>{t("update.install")}</button>
    </div>
  {/if}

  {#if missingTool}
    <Section title={t("setup.title")}>
      {#each session.tools as status (status.tool)}
        <ToolRow {status} onchange={replaceTool} />
      {/each}
    </Section>
  {/if}

  {#if issue}
    <p class="issue" title={issue.message}>
      {t(`projects.issue.${issue.kind}`, {}, "projects.issue.other")}
    </p>
  {/if}

  {#each groups as group (group.key)}
    <Section title={group.title}>
      {#each group.projects as project (project.id)}
        <ProjectRow {project} {now} unseen={unseen.has(project.id)} />
      {/each}
      {#each group.unaccepted as item (item.id)}
        <UnacceptedRow {item} {now} unseen={unseen.has(item.id)} />
      {/each}
    </Section>
  {:else}
    <p class="empty">
      {#if query && (allProjects.length > 0 || session.snapshot?.unaccepted.length)}
        {t("projects.noMatch")}
      {:else if syncing}
        {t("projects.loading")}
      {:else}
        {t("projects.empty")}
        <button onclick={() => goto("/settings")}>{t("app.settings")}</button>
      {/if}
    </p>
  {/each}
</main>

<footer>
  <LoginStatus auth={session.auth} {ghInstalled} onchange={signedIn} />
  {#if syncedLabel && syncedAt && !session.signingIn}
    <span class="synced" title={syncedAt.toLocaleString()}>{syncedLabel}</span>
  {/if}
</footer>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px 4px;
    position: relative;
  }

  /* A hairline under the titlebar; the spinning refresh icon stays the main cue. */
  .progress {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: -4px;
    height: 2px;
    border-radius: 1px;
    overflow: hidden;
    background: var(--bg-control);
  }

  .progress > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s ease-out;
  }

  .progress.indeterminate > div {
    width: 30%;
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

  .search {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    height: 26px;
    font: inherit;
    font-size: 0.9em;
    color: inherit;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  .search:focus-visible {
    border-color: var(--accent);
  }

  .tools {
    display: flex;
    gap: 4px;
  }

  .spinning svg {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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
    padding: var(--space) 12px;
  }

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
    border-radius: var(--radius-card);
    background: var(--bg-card);
    border: 1px solid var(--border);
  }

  .issue {
    margin: 0;
    padding: 0 4px;
    font-size: 0.85em;
    color: var(--warning);
  }

  .empty {
    margin: 0;
    padding: 24px 12px;
    color: var(--fg-muted);
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    /* Holds its height while the login status loads, so it never collapses. Same side
       padding as the list, so the account pill lines up with the cards. */
    min-height: 26px;
    padding: 6px 12px;
    font-size: 0.85em;
    color: var(--fg-muted);
    border-top: 1px solid var(--border);
  }

  .synced {
    flex-shrink: 0;
    padding-right: 4px;
    white-space: nowrap;
  }
</style>
