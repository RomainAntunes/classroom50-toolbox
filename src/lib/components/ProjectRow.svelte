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
  const url = $derived(project.remote?.url ?? null);
</script>

<div class="row">
  <div class="text">
    <span class="name" title={project.repo ?? path ?? undefined}>{name}</span>
    <span class="meta">
      {project.config.classroom}
      {#if dueLabel}
        · <span class:soon={dueSoon}>{dueLabel}</span>
      {/if}
      {#if !path}
        · {t("projects.notCloned")}
      {/if}
    </span>
  </div>
  {#if closed}
    <span class="pill">{t("projects.closed")}</span>
  {:else if project.pending}
    <span class="pill warn">{t(`projects.pending.${project.pending}`)}</span>
  {:else if behind > 0}
    <span class="pill">{t("projects.behind", { count: behind })}</span>
  {/if}
  {#if path}
    <button title={t("projects.openFolder")} onclick={() => revealItemInDir(path)}>⧉</button>
    <button title={t("projects.openTerminal")} onclick={() => api.openTerminal(path)}>
      &gt;_
    </button>
  {/if}
  {#if url}
    <button title={t("projects.openGitHub")} onclick={() => openUrl(url)}>↗</button>
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding: 8px 12px;
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
