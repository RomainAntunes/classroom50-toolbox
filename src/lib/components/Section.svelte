<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "$lib/i18n";
  import { menuKey, onContextMenu } from "$lib/keyboard";

  let {
    title,
    actions,
    children,
    collapsed = false,
    list = false,
    ontoggle,
    onmenu,
    note,
  }: {
    title: string;
    actions?: Snippet;
    children: Snippet;
    collapsed?: boolean;
    /** The rows are list items (`<li>`), announced as a list. */
    list?: boolean;
    /** Makes the title fold and unfold the section. */
    ontoggle?: () => void;
    /** Adds a "..." button (and the title's context menu) opening a menu, under `anchor`
     *  when given, else at the pointer. */
    onmenu?: (anchor?: HTMLElement) => void;
    /** Shown after the title, muted (a count while folded, "Hidden"). */
    note?: string;
  } = $props();
</script>

<section>
  <header>
    <!-- The fold button sits inside the heading, so the heading stays one for screen readers. -->
    <h2>
      {#if ontoggle}
        <button
          class="title"
          aria-expanded={!collapsed}
          onclick={ontoggle}
          oncontextmenu={onmenu && onContextMenu(() => onmenu())}
          onkeydown={onmenu && menuKey(onmenu)}
        >
          <svg class:open={!collapsed} viewBox="0 0 24 24" aria-hidden="true"
            ><path d="m9 18 6-6-6-6" /></svg
          >
          <span class="label">{title}</span>
          {#if note}<span class="note">{note}</span>{/if}
        </button>
      {:else}
        <span class="label">{title}</span>{#if note}<span class="note">{note}</span>{/if}
      {/if}
    </h2>
    {@render actions?.()}
    {#if onmenu}
      <button
        class="more"
        aria-label={t("section.more", { title })}
        aria-haspopup="menu"
        title={t("section.more", { title })}
        onclick={(event) => onmenu(event.currentTarget)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle
            cx="19"
            cy="12"
            r="1.6"
          />
        </svg>
      </button>
    {/if}
  </header>
  {#if !collapsed}
    {#if list}
      <!-- An explicit role: WebKit drops the list semantics of a list without bullets. -->
      <!-- svelte-ignore a11y_no_redundant_roles -->
      <ul class="card" role="list">
        {@render children()}
      </ul>
    {:else}
      <div class="card">
        {@render children()}
      </div>
    {/if}
  {/if}
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 0.75);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 0 4px;
  }

  h2 {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    margin: 0;
    font: inherit;
  }

  .label {
    font-size: 0.85em;
    font-weight: 600;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .note {
    margin-left: 6px;
    font-size: 0.72em;
    font-weight: 400;
    color: var(--fg-muted);
    white-space: nowrap;
  }

  .title .note {
    font-size: 0.85em;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    flex: 1;
    padding: 3px 8px 3px 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .title svg {
    flex: none;
    width: 10px;
    height: 10px;
    fill: none;
    stroke: var(--fg-muted);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: transform 0.15s ease-out;
  }

  .title svg.open {
    transform: rotate(90deg);
  }

  .more {
    flex: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    opacity: 0.55;
    cursor: default;
  }

  .more:hover,
  .more:focus-visible {
    opacity: 1;
    background: var(--bg-control-hover);
  }

  .more svg {
    width: 14px;
    height: 14px;
    fill: var(--fg-muted);
  }

  .card {
    margin: 0;
    padding: 0;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    overflow: hidden;
  }

  .card > :global(* + *) {
    border-top: 1px solid var(--border);
  }
</style>
