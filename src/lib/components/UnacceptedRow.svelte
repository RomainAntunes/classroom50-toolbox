<script lang="ts">
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { withDialog, type Unaccepted } from "$lib/api";
  import { isHidden, setHidden } from "$lib/hidden";
  import { acceptAssignment, acceptBlocked } from "$lib/accept";
  import { session } from "$lib/session.svelte";
  import { acceptLink, copyLink } from "$lib/links";
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

  const blocked = $derived(acceptBlocked(item, now));

  const accepting = $derived(session.accepting.includes(item.id));
  function accept() {
    void acceptAssignment(item);
  }

  const hidden = $derived(isHidden(item.id));

  async function showMenu(event: MouseEvent) {
    event.preventDefault();
    const items = await Promise.all([
      MenuItem.new({
        text: t("projects.accept"),
        enabled: !accepting && blocked === null,
        action: () => accept(),
      }),
      MenuItem.new({
        text: t("projects.openAcceptPage"),
        action: () => openUrl(item.acceptUrl),
      }),
      MenuItem.new({
        text: t("projects.copyLink"),
        action: () => copyLink(acceptLink(item)),
      }),
      PredefinedMenuItem.new({ item: "Separator" }),
      MenuItem.new({
        text: t(hidden ? "projects.unhide" : "projects.hide"),
        action: () => setHidden(item.id, !hidden),
      }),
    ]);
    const menu = await Menu.new({ items });
    await withDialog(() => menu.popup());
  }

  let open = $state(false);

  // A link pointed here: open the details and bring the row into view.
  let element: HTMLElement | undefined = $state();
  const linked = $derived(session.focus?.id === item.id ? session.focus.seq : null);
  $effect(() => {
    if (linked === null) return;
    open = true;
    element?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  });
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
<div class="item" class:linked={linked !== null} bind:this={element}>
  <div
    class="row"
    role="button"
    tabindex="0"
    aria-expanded={open}
    aria-label={item.assignment.name}
    oncontextmenu={showMenu}
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
        {#if hidden}
          <span class="hidden-tag">{t("projects.hidden")}</span> ·
        {/if}
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
  /* The row a link pointed to. */
  .item.linked {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .hidden-tag {
    font-style: italic;
  }

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
