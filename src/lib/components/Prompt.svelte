<script lang="ts">
  import { t } from "#lib/i18n/index.js";
  import { prompt } from "#lib/prompt.svelte.js";
  import Modal from "./Modal.svelte";

  let value = $state("");
  // A read-only preview (`showText`) has no input; its main action is always available.
  const preview = $derived(prompt.current?.text !== undefined);
  const ready = $derived(preview || value.trim() !== "");

  // A prompt that replaces another keeps the shell (and the focus), with an empty field.
  $effect(() => {
    if (prompt.current) value = "";
  });

  function close(result: string | null, alt = false) {
    prompt.current?.resolve(result, alt);
  }
</script>

{#if prompt.current}
  {@const current = prompt.current}
  <Modal
    labelledby="prompt-title"
    describedby={current.message ? "prompt-message" : undefined}
    onclose={() => close(null)}
  >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        if (ready) close(preview ? "ok" : value);
      }}
    >
      <h2 id="prompt-title"><bdi>{current.title}</bdi></h2>
      {#if current.message}<p id="prompt-message">{current.message}</p>{/if}
      {#if current.text !== undefined}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <pre
          tabindex="0"
          aria-labelledby="prompt-title"
          dir="ltr"
          data-autofocus>{current.text}</pre>
      {:else}
        <input
          bind:value
          aria-label={current.label ?? current.title}
          placeholder={current.placeholder}
          maxlength={current.maxLength}
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          data-autofocus
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
  </Modal>
{/if}

<style>
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
