<script lang="ts">
  import { t } from "$lib/i18n";
  import { prompt } from "$lib/prompt.svelte";

  let value = $state("");
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (prompt.current) {
      value = "";
      input?.focus();
    }
  });

  function close(result: string | null) {
    prompt.current?.resolve(result);
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
          close(value);
        }}
      >
        <h2 id="prompt-title">{current.title}</h2>
        {#if current.message}<p>{current.message}</p>{/if}
        <input
          bind:this={input}
          bind:value
          placeholder={current.placeholder}
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
        />
        <div class="buttons">
          <button type="button" onclick={() => close(null)}>{t("prompt.cancel")}</button>
          <button type="submit" class="primary" disabled={!value.trim()}>{current.okLabel}</button>
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
    max-width: 320px;
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

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
