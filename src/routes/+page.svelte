<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    PROJECTS_UPDATED_EVENT,
    UPDATE_AVAILABLE_EVENT,
    type AuthStatus,
    type Snapshot,
    type ToolStatus,
    type UpdateInfo,
  } from "$lib/api";
  import { t } from "$lib/i18n";
  import { loadSettings, pickWatchedFolder } from "$lib/settings.svelte";
  import Section from "$lib/components/Section.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";
  import ProjectRow from "$lib/components/ProjectRow.svelte";
  import LoginStatus from "$lib/components/LoginStatus.svelte";

  let tools = $state<ToolStatus[]>([]);
  let auth = $state<AuthStatus | null>(null);
  let snapshot = $state<Snapshot | null>(null);
  let now = $state(new Date());
  let update = $state<UpdateInfo | null>(null);
  let loading = $state(false);

  const missingTool = $derived(tools.some((tool) => !tool.installed));
  const ghInstalled = $derived(tools.some((tool) => tool.tool === "gh" && tool.installed));
  // Signing in is already offered in the footer.
  const issue = $derived(snapshot?.issue?.kind === "notSignedIn" ? null : snapshot?.issue);
  const syncing = $derived(loading || snapshot?.syncedAt == null);

  async function refresh() {
    loading = true;
    try {
      await loadSettings();
      [tools, auth, snapshot] = await Promise.all([
        api.toolStatuses(),
        api.authStatus(),
        api.refreshProjects(),
      ]);
      now = new Date();
    } finally {
      loading = false;
    }
  }

  async function addFolder() {
    if (await pickWatchedFolder()) snapshot = await api.refreshProjects();
  }

  function replaceTool(next: ToolStatus) {
    tools = tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }

  async function signedIn(next: AuthStatus) {
    auth = next;
    snapshot = await api.refreshProjects();
  }

  onMount(() => {
    // Show the last sync right away; `refresh` replaces it once the new one lands.
    api.listProjects().then((last) => (snapshot ??= last));
    refresh();
    const unlisteners = [
      listen<UpdateInfo>(UPDATE_AVAILABLE_EVENT, (event) => (update = event.payload)),
      listen<Snapshot>(PROJECTS_UPDATED_EVENT, (event) => {
        snapshot = event.payload;
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
  <div class="tools">
    <button disabled={loading} onclick={refresh} title={t("app.refresh")}>↻</button>
    <button onclick={() => goto("/settings")} title={t("app.settings")}>⚙</button>
  </div>
</header>

<main>
  {#if update}
    <div class="banner">
      <span>{t("update.available", { version: update.version })}</span>
      <button class="primary" onclick={() => api.installUpdate()}>{t("update.install")}</button>
    </div>
  {/if}

  {#if missingTool}
    <Section title={t("setup.title")}>
      {#each tools as status (status.tool)}
        <ToolRow {status} onchange={replaceTool} />
      {/each}
    </Section>
  {/if}

  <Section title={t("projects.title")}>
    {#snippet actions()}
      <button onclick={addFolder}>{t("projects.addFolder")}</button>
    {/snippet}
    {#if issue}
      <p class="issue" title={issue.message}>
        {t(`projects.issue.${issue.kind}`, {}, "projects.issue.other")}
      </p>
    {/if}
    {#each snapshot?.projects ?? [] as project (project.id)}
      <ProjectRow {project} {now} />
    {:else}
      <p class="empty">{t(syncing ? "projects.loading" : "projects.empty")}</p>
    {/each}
  </Section>
</main>

<footer>
  <LoginStatus {auth} {ghInstalled} onchange={signedIn} />
</footer>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px 4px;
  }

  .tools {
    display: flex;
    gap: 4px;
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
    padding: 8px 12px;
    font-size: 0.85em;
    color: var(--warning);
    border-bottom: 1px solid var(--border);
  }

  .empty {
    margin: 0;
    padding: 12px;
    color: var(--fg-muted);
    text-align: center;
  }

  footer {
    padding: 8px 14px 10px;
    font-size: 0.85em;
    color: var(--fg-muted);
    border-top: 1px solid var(--border);
  }
</style>
