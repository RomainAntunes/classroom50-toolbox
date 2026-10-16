<script lang="ts">
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Unaccepted } from "$lib/api";
  import { menuKey, onContextMenu, popupMenu } from "$lib/keyboard";
  import { isHidden, setHidden } from "$lib/hidden";
  import { acceptAssignment, acceptBlocked } from "$lib/accept";
  import { session } from "$lib/session.svelte";
  import { acceptLink, copyLink } from "$lib/links";
  import { groupDetails, groupName } from "$lib/group";
  import { formatDate, fromNow, t } from "$lib/i18n";
  import Details from "./Details.svelte";

  let { item, now, unseen }: { item: Unaccepted; now: Date; unseen: boolean } = $props();
  const uid = $props.id();

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

  /** The row's native menu, under `anchor` or at the pointer. */
  async function showMenu(anchor?: HTMLElement) {
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
    await popupMenu(menu, anchor);
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
<li class="item" class:linked={linked !== null} data-item bind:this={element}>
  <!-- A right click anywhere on the row opens its menu; the keyboard opens it from the
       row's button (Menu key or Shift+F10). -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="row" oncontextmenu={onContextMenu(() => showMenu())}>
    <button
      class="toggle"
      data-row
      aria-expanded={open}
      aria-labelledby="{uid}-name"
      aria-describedby="{uid}-meta"
      onclick={() => (open = !open)}
      onkeydown={menuKey((anchor) => showMenu(anchor))}
    >
      <span class="text">
        <span class="name" id="{uid}-name">
          {#if unseen}<span class="dot" aria-hidden="true"></span>{/if}{item.assignment
            .name}{#if unseen}<span class="sr-only">, {t("projects.new")}</span>{/if}
        </span>
        <span class="meta" id="{uid}-meta">
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
          <!-- The disabled Accept button can't take focus, so its hint is read here. -->
          {#if blocked}<span class="sr-only">. {blocked}</span>{/if}
        </span>
      </span>
    </button>
    <button
      class="primary"
      title={blocked ?? undefined}
      disabled={accepting || blocked !== null}
      onclick={accept}>{t("projects.accept")}</button
    >
    <button
      title={t("projects.openAcceptPage")}
      aria-label={t("projects.openAcceptPage")}
      onclick={() => openUrl(item.acceptUrl)}><span aria-hidden="true">↗</span></button
    >
  </div>
  {#if open}
    <Details description={item.assignment.description} items={details} />
  {/if}
</li>

<style>
  /* The row a link pointed to. */
  .item.linked {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .hidden-tag {
    font-style: italic;
  }

  .item {
    list-style: none;
  }

  /* The button that opens the details fills the row up to its action buttons. */
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding-right: 12px;
    transition: background-color 0.12s;
  }

  .row:hover {
    background: var(--bg-control);
  }

  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    padding: 8px 0 8px 12px;
    border-radius: 0;
    background: none;
    text-align: left;
    white-space: normal;
  }

  .toggle:hover:not(:disabled) {
    background: none;
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

  .toggle:focus-visible {
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
