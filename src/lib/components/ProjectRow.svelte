<script lang="ts">
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, withDialog, type Project } from "$lib/api";
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
  // One status at most, most important first; `warn` ones need the student's action.
  const status = $derived.by(() => {
    if (closed) return { label: t("projects.closed"), warn: false };
    if (project.pending) return { label: t(`projects.pending.${project.pending}`), warn: true };
    if (behind > 0) return { label: t("projects.behind", { count: behind }), warn: false };
    return null;
  });
  const path = $derived(project.path);
  const githubUrl = $derived(
    project.remote?.url ??
      (project.owner && project.repo
        ? `https://github.com/${project.owner}/${project.repo}`
        : null),
  );
  const openTarget = $derived(project.webUrl ?? githubUrl);

  /** The row's native context menu with every action. */
  async function showMenu(event: MouseEvent) {
    event.preventDefault();
    const os = document.documentElement.dataset.os ?? "linux";
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const items = await Promise.all([
      MenuItem.new({
        text: t("projects.openClassroom50"),
        enabled: Boolean(project.webUrl),
        action: () => project.webUrl && openUrl(project.webUrl),
      }),
      MenuItem.new({
        text: t("projects.openGitHub"),
        enabled: Boolean(githubUrl),
        action: () => githubUrl && openUrl(githubUrl),
      }),
      separator(),
      MenuItem.new({
        text: t(`projects.openFolder.${os}`, {}, "projects.openFolder.linux"),
        enabled: Boolean(path),
        action: () => path && revealItemInDir(path),
      }),
      MenuItem.new({
        text: t("projects.openTerminal"),
        enabled: Boolean(path),
        action: () => path && api.openTerminal(path),
      }),
    ]);
    const menu = await Menu.new({ items });
    // Pinned like a dialog, so the popover stays open while the menu is up.
    await withDialog(() => menu.popup());
  }
</script>

<div class="row" role="group" aria-label={name} oncontextmenu={showMenu}>
  <div class="text">
    <span class="name" title={project.repo ?? path ?? undefined}>{name}</span>
    <span class="meta">
      {#if status}
        <span class="status" class:warn={status.warn}>{status.label}</span> ·
      {/if}
      {#if dueLabel}
        <span class:soon={dueSoon}>{dueLabel}</span> ·
      {/if}
      {path ? (project.repo ?? project.config.assignment) : t("projects.notCloned")}
    </span>
  </div>
  {#if openTarget}
    <button
      title={t(project.webUrl ? "projects.openClassroom50" : "projects.openGitHub")}
      onclick={() => openUrl(openTarget)}>↗</button
    >
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

  .status {
    font-weight: 500;
  }

  .status.warn {
    color: var(--warning);
  }
</style>
