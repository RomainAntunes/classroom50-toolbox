<script lang="ts">
  import { t } from "$lib/i18n";
  import { prompt } from "$lib/prompt.svelte";

  let value = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let pre = $state<HTMLPreElement | null>(null);
  // A read-only preview (`showText`) has no input; its main action is always available.
  const preview = $derived(prompt.current?.text !== undefined);
  const ready = $derived(preview || value.trim() !== "");

  $effect(() => {
    if (prompt.current) {
      value = "";
      (input ?? pre)?.focus();
    }
  });

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
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => {
        if (event.key === "Escape") close(null);
      }}
    >
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (ready) close(preview ? "ok" : value);
        }}
      >
        <h2 id="prompt-title">{current.title}</h2>
        {#if current.message}<p>{current.message}</p>{/if}
        {#if current.text !== undefined}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre bind:this={pre} tabindex="0">{current.text}</pre>
        {:else}
          <input
            bind:this={input}
            bind:value
            placeholder={current.placeholder}
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
    border-color: var(--accent);
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
    border-color: var(--accent);
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
