<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Unaccepted } from "$lib/api";
  import { fromNow, t } from "$lib/i18n";

  let { item, now, unseen }: { item: Unaccepted; now: Date; unseen: boolean } = $props();

  const due = $derived(item.assignment.due ? new Date(item.assignment.due) : null);
  const dueLabel = $derived(
    due === null
      ? null
      : t(due > now ? "projects.due" : "projects.wasDue", { when: fromNow(due, now) }),
  );
</script>

<!-- A published assignment with no repo yet; accepting happens on Classroom 50. -->
<div class="row">
  <div class="text">
    <span class="name">
      {#if unseen}<span class="dot" aria-label={t("projects.new")}></span>{/if}{item.assignment
        .name}
    </span>
    <span class="meta">
      <span class="status">{t("projects.notAccepted")}</span>
      {#if dueLabel}
        · {dueLabel}
      {/if}
    </span>
  </div>
  <button title={t("projects.accept")} onclick={() => openUrl(item.acceptUrl)}>↗</button>
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
    color: var(--fg-muted);
  }

  .meta {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .status {
    font-weight: 500;
    color: var(--accent);
  }

  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: 1px;
  }
</style>
