<script lang="ts">
  import { t } from "$lib/i18n";
  import { prompt } from "$lib/prompt.svelte";

  let value = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let pre = $state<HTMLPreElement | null>(null);
  // A read-only preview (`showText`) has no input; its main action is always available.
  const preview = $derived(prompt.current?.text !== undefined);
  const ready = $derived(preview || value.trim() !== "");
  let card = $state<HTMLElement | null>(null);
  /** What had the focus before the prompt opened; it gets it back when the prompt closes. */
  let restore: HTMLElement | null = null;

  $effect(() => {
    if (prompt.current) {
      restore ??= document.activeElement instanceof HTMLElement ? document.activeElement : null;
      value = "";
      (input ?? pre)?.focus();
    } else if (restore) {
      const previous = restore;
      restore = null;
      if (previous.isConnected) previous.focus();
    }
  });

  /** Keeps Tab and Shift+Tab inside the prompt, and Escape cancels it. */
  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      close(null);
      return;
    }
    if (event.key !== "Tab" || !card) return;
    const focusable = [
      ...card.querySelectorAll<HTMLElement>("input, pre[tabindex], button:not(:disabled)"),
    ];
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

  function close(result: string | null, alt = false) {
    prompt.current?.resolve(result, alt);
  }
</script>

{#if prompt.current}
  {@const current = prompt.current}
  <div class="scrim" role="presentation" onclick={() => close(null)}>
    <div
      class="card"
      role="dialog"
      aria-modal="true"
      aria-labelledby="prompt-title"
      aria-describedby={current.message ? "prompt-message" : undefined}
      tabindex="-1"
      bind:this={card}
      onclick={(event) => event.stopPropagation()}
      onkeydown={onKeyDown}
    >
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (ready) close(preview ? "ok" : value);
        }}
      >
        <h2 id="prompt-title">{current.title}</h2>
        {#if current.message}<p id="prompt-message">{current.message}</p>{/if}
        {#if current.text !== undefined}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre bind:this={pre} tabindex="0" aria-labelledby="prompt-title">{current.text}</pre>
        {:else}
          <input
            bind:this={input}
            bind:value
            aria-label={current.title}
            placeholder={current.placeholder}
            maxlength={current.maxLength}
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
          />
        {/if}
        <div class="buttons">
          <button type="button" onclick={() => close(null)}>{t("prompt.cancel")}</button>
          {#if current.altLabel}
            <button type="button" disabled={!value.trim()} onclick={() => close(value, true)}
              >{current.altLabel}</button
            >
          {/if}
          <button type="submit" class="primary" disabled={!ready}>{current.okLabel}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

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
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg-card);
    backdrop-filter: blur(30px) saturate(1.6);
    -webkit-backdrop-filter: blur(30px) saturate(1.6);
    box-shadow: 0 8px 30px rgb(0 0 0 / 0.25);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }

  h2 {
    margin: 0;
    font-size: 1em;
    font-weight: 600;
  }

  p {
    margin: 0;
    color: var(--fg-muted);
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  input {
    box-sizing: border-box;
    height: 28px;
    font: inherit;
    color: inherit;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  input:focus-visible {
    border-color: var(--focus-ring);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }

  pre {
    margin: 0;
    max-height: 240px;
    overflow: auto;
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 0.8em;
    line-height: 1.4;
    white-space: pre;
    user-select: text;
    -webkit-user-select: text;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  pre:focus-visible {
    border-color: var(--focus-ring);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
