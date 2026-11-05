<script lang="ts">
  // The shell of an in-popover dialog (the prompt, the phases editor): a scrim that cancels
  // on a click, a card that keeps Tab inside it, Escape to cancel, and the focus back where
  // it was once it closes. Render it while open: `{#if open}<Modal ...>{/if}`. It focuses
  // the first `[data-autofocus]` inside, else the card.
  import { onMount, type Snippet } from "svelte";
  import { modals } from "#lib/modal.svelte.js";

  let {
    labelledby,
    describedby,
    onclose,
    children,
  }: {
    labelledby: string;
    describedby?: string;
    onclose: () => void;
    children: Snippet;
  } = $props();

  let card = $state<HTMLElement | null>(null);

  onMount(() => {
    const restore = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    modals.open += 1;
    (card?.querySelector<HTMLElement>("[data-autofocus]") ?? card)?.focus();
    return () => {
      modals.open -= 1;
      if (restore?.isConnected) restore.focus();
    };
  });

  const FOCUSABLE =
    "input:not(:disabled), select:not(:disabled), textarea:not(:disabled), pre[tabindex], button:not(:disabled)";

  /** Keeps Tab and Shift+Tab inside the card, and Escape cancels. */
  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
      return;
    }
    if (event.key !== "Tab" || !card) return;
    const focusable = [...card.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    // The handler only runs with the focus in the card; on the card itself (a click on
    // its padding) Tab would otherwise leave it.
    const onCard = document.activeElement === card;
    if (event.shiftKey && (document.activeElement === first || onCard)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (document.activeElement === last || onCard)) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}>
  <div
    class="card"
    role="dialog"
    aria-modal="true"
    aria-labelledby={labelledby}
    aria-describedby={describedby}
    tabindex="-1"
    bind:this={card}
    onclick={(event) => event.stopPropagation()}
    onkeydown={onKeyDown}
  >
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    background: rgb(0 0 0 / 0.3);
    border-radius: var(--radius-window);
    z-index: 10;
  }

  .card {
    width: 100%;
    /* Three buttons (Cancel, Commit only, Commit & push) fit on one line at 14px; the
       380px popover minus the scrim's padding leaves 348px. */
    max-width: 340px;
    /* A long message (accept's summary) scrolls inside the popover. */
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg-card);
    backdrop-filter: blur(30px) saturate(1.6);
    -webkit-backdrop-filter: blur(30px) saturate(1.6);
    box-shadow: 0 8px 30px rgb(0 0 0 / 0.25);
  }
</style>
