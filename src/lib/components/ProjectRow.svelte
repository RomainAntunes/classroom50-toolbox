<script lang="ts">
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type Project } from "$lib/api";
  import { fromNow, t } from "$lib/i18n";

  let { project, now }: { project: Project; now: Date } = $props();

  const DAY = 24 * 60 * 60 * 1000;

  const name = $derived(project.assignment?.name ?? project.config.assignment);
  const closed = $derived(Boolean(project.assignment?.closed || project.remote?.archived));
  const due = $derived(project.assignment?.due ? new Date(project.assignment.due) : null);
  const dueSoon = $derived(
    due !== null && !closed && due > now && due.getTime() - now.getTime() < DAY,
  );
  const dueLabel = $derived(
    due === null
      ? null
      : t(due > now ? "projects.due" : "projects.wasDue", { when: fromNow(due, now) }),
  );
  const behind = $derived(project.local?.behind ?? 0);
  const path = $derived(project.path);
  const githubUrl = $derived(
    project.remote?.url ??
      (project.owner && project.repo
        ? `https://github.com/${project.owner}/${project.repo}`
        : null),
  );
  const webUrl = $derived(project.webUrl);
</script>

<div class="row">
  <div class="text">
    <span class="name" title={project.repo ?? path ?? undefined}>{name}</span>
    <span class="meta">
      {#if dueLabel}
        <span class:soon={dueSoon}>{dueLabel}</span> ·
      {/if}
      {path ? (project.repo ?? project.config.assignment) : t("projects.notCloned")}
    </span>
  </div>
  {#if closed}
    <span class="pill">{t("projects.closed")}</span>
  {:else if project.pending}
    <span class="pill warn">{t(`projects.pending.${project.pending}`)}</span>
  {:else if behind > 0}
    <span class="pill">{t("projects.behind", { count: behind })}</span>
  {/if}
  <!-- Icons from Lucide (ISC). -->
  {#if path}
    <button
      class="icon"
      title={t("projects.openFolder")}
      aria-label={t("projects.openFolder")}
      onclick={() => revealItemInDir(path)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"
        />
      </svg>
    </button>
    <button
      class="icon"
      title={t("projects.openTerminal")}
      aria-label={t("projects.openTerminal")}
      onclick={() => api.openTerminal(path)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="m7 11 2-2-2-2" />
        <path d="M11 13h4" />
        <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
      </svg>
    </button>
  {/if}
  {#if githubUrl}
    <button
      class="icon"
      title={t("projects.openGitHub")}
      aria-label={t("projects.openGitHub")}
      onclick={() => openUrl(githubUrl)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"
        />
        <path d="M9 18c-4.51 2-5-2-7-2" />
      </svg>
    </button>
  {/if}
  {#if webUrl}
    <button
      class="icon"
      title={t("projects.openClassroom50")}
      aria-label={t("projects.openClassroom50")}
      onclick={() => openUrl(webUrl)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M15 3h6v6" />
        <path d="M10 14 21 3" />
        <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
      </svg>
    </button>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding: 8px 12px;
    transition: background-color 0.12s;
  }

  .row:hover {
    background: var(--bg-control);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-weight: 500;
  }

  .meta {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .soon {
    color: var(--warning);
  }

  .pill {
    flex-shrink: 0;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 0.8em;
    color: var(--fg-muted);
    border: 1px solid var(--border);
  }

  .pill.warn {
    color: var(--warning);
    border-color: currentColor;
  }
</style>
