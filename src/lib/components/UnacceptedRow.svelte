<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Unaccepted } from "$lib/api";
  import { acceptAssignment } from "$lib/accept";
  import { groupDetails, groupName } from "$lib/group";
  import { formatDate, fromNow, t } from "$lib/i18n";
  import Details from "./Details.svelte";

  let { item, now, unseen }: { item: Unaccepted; now: Date; unseen: boolean } = $props();

  const due = $derived(item.assignment.due ? new Date(item.assignment.due) : null);
  const dueLabel = $derived(
    due === null
      ? null
      : t(due > now ? "projects.due" : "projects.wasDue", { when: fromNow(due, now) }),
  );
  const opens = $derived(
    item.assignment.availableFrom ? new Date(item.assignment.availableFrom) : null,
  );
  const upcoming = $derived(opens !== null && opens > now);

  const blocked = $derived(
    item.assignment.locked
      ? t("projects.lockedHint")
      : upcoming
        ? t("projects.upcomingHint")
        : null,
  );

  let accepting = $state(false);
  async function accept() {
    accepting = true;
    try {
      await acceptAssignment(item);
    } finally {
      accepting = false;
    }
  }

  let open = $state(false);
  const details = $derived.by(() => {
    const a = item.assignment;
    const items: [string, string][] = [
      [t("details.type"), t(`details.mode.${a.mode}`)],
      [t("details.submission"), t(`details.submissionMode.${a.submissionMode ?? "every-push"}`)],
    ];
    if (opens) items.push([t("details.opens"), formatDate(opens)]);
    if (due) items.push([t("details.due"), formatDate(due)]);
    if (item.group) items.push(...groupDetails(item.group));
    return items;
  });
</script>

<!-- An assignment with no repo yet, published or upcoming. -->
<div class="item">
  <div
    class="row"
    role="button"
    tabindex="0"
    aria-expanded={open}
    aria-label={item.assignment.name}
    onclick={() => (open = !open)}
    onkeydown={(event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        open = !open;
      }
    }}
  >
    <div class="text">
      <span class="name">
        {#if unseen}<span class="dot" aria-label={t("projects.new")}></span>{/if}{item.assignment
          .name}
      </span>
      <span class="meta">
        <span class="status" class:upcoming
          >{accepting
            ? t("projects.accepting")
            : upcoming && opens
              ? t("projects.opens", { when: fromNow(opens, now) })
              : t("projects.notAccepted")}</span
        >
        {#if item.group}
          · {t("projects.inGroup", { group: groupName(item.group) })}
        {/if}
        {#if dueLabel}
          · {dueLabel}
        {/if}
      </span>
    </div>
    <button
      class="primary"
      title={blocked ?? undefined}
      disabled={accepting || blocked !== null}
      onclick={(event) => {
        event.stopPropagation();
        accept();
      }}>{t("projects.accept")}</button
    >
    <button
      title={t("projects.openAcceptPage")}
      onclick={(event) => {
        event.stopPropagation();
        openUrl(item.acceptUrl);
      }}>↗</button
    >
  </div>
  {#if open}
    <Details description={item.assignment.description} items={details} />
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

  .status.upcoming {
    color: var(--fg-muted);
  }

  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
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
