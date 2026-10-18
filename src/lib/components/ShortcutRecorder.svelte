<script lang="ts">
  // Settings > General > Keyboard shortcut: click, press a combination, it saves (Rust
  // registers it first and refuses one another app holds); Escape cancels, Clear turns it
  // off. While it listens the saved shortcut is unregistered, so pressing it again reaches
  // the recorder instead of hiding the popover.
  import { onDestroy } from "svelte";
  import { api, errorMessage, type Os } from "#lib/api.js";
  import { announce } from "#lib/announce.svelte.js";
  import { t } from "#lib/i18n/index.js";
  import { saveSettings, settings } from "#lib/settings.svelte.js";
  import {
    formatAccelerator,
    formatShortcut,
    heldModifiers,
    recordKey,
    type Modifier,
  } from "#lib/shortcut.js";

  let { os, supported }: { os: Os; supported: boolean } = $props();

  const word = (name: string) => t(`shortcut.key.${name}`);
  const saved = $derived(settings.current?.globalShortcut ?? null);
  const savedLabel = $derived(saved && (formatAccelerator(saved, os, word) ?? saved));

  let recording = $state(false);
  let held = $state<Modifier[]>([]);
  // A hint while recording, or why a shortcut wasn't saved.
  let problem = $state<string | null>(null);
  // Registered now; differs from `saved` when another app took it since it was saved.
  let active = $state<string | null>(null);
  let saving = false;
  let button = $state<HTMLButtonElement>();

  async function refreshActive() {
    active = supported ? await api.activeGlobalShortcut().catch(() => null) : null;
  }

  $effect(() => {
    void saved;
    void refreshActive();
  });

  const inactive = $derived(supported && !recording && saved !== null && active !== saved);

  async function start() {
    problem = null;
    held = [];
    recording = true;
    // WebKit doesn't focus a clicked button, and the keys go to the focused element.
    button?.focus();
    await api.pauseGlobalShortcut(true).catch(() => undefined);
  }

  async function stop() {
    if (!recording) return;
    recording = false;
    held = [];
    await api.pauseGlobalShortcut(false).catch(() => undefined);
    await refreshActive();
  }

  async function onkeydown(event: KeyboardEvent) {
    if (!recording) return;
    const plain = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    // Tab leaves (the blur stops recording), so the keyboard is never trapped here.
    if (event.key === "Tab" && plain) return;
    // Before any await, so the layout doesn't hide the popover on Escape.
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && plain) {
      problem = null;
      await stop();
      return;
    }
    if (event.repeat || saving) return;
    const result = recordKey(event, os);
    switch (result.kind) {
      case "modifiers":
        held = result.held;
        problem = null;
        return;
      case "needsModifier":
        held = result.held;
        problem = t("settings.shortcut.needsModifier");
        return;
      case "unsupported":
        held = result.held;
        problem = t("settings.shortcut.unsupportedKey");
        return;
    }
    saving = true;
    try {
      await saveSettings({ globalShortcut: result.accelerator });
      problem = null;
      await stop();
      const label = formatAccelerator(result.accelerator, os, word) ?? result.accelerator;
      announce(t("settings.shortcut.saved", { shortcut: label }));
    } catch (err) {
      // Not saved: still listening for another one.
      held = [];
      problem = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  function onkeyup(event: KeyboardEvent) {
    if (recording) held = heldModifiers(event);
  }

  async function clear() {
    problem = null;
    try {
      await saveSettings({ globalShortcut: null });
      announce(t("settings.shortcut.cleared"));
    } catch (err) {
      problem = errorMessage(err);
    }
  }

  onDestroy(() => {
    if (recording) void api.pauseGlobalShortcut(false);
  });

  const shown = $derived(
    recording
      ? held.length
        ? formatShortcut(held, null, os, word)
        : t("settings.shortcut.press")
      : (savedLabel ?? t("settings.shortcut.record")),
  );
</script>

<div class="row">
  <span class="text">
    <span>{t("settings.shortcut")}</span>
    <span class="sub" id="shortcut-hint"
      >{t(supported ? "settings.shortcut.hint" : "settings.shortcut.wayland")}</span
    >
  </span>
  <span class="controls">
    <button
      class="recorder"
      class:recording
      bind:this={button}
      disabled={!supported}
      aria-pressed={recording}
      aria-label={`${t("settings.shortcut")}: ${shown}`}
      aria-describedby={problem || inactive ? "shortcut-hint shortcut-problem" : "shortcut-hint"}
      onclick={() => void (recording ? stop() : start())}
      {onkeydown}
      {onkeyup}
      onblur={() => void stop()}>{shown}</button
    >
    {#if saved && !recording}
      <button onclick={clear}>{t("settings.shortcut.clear")}</button>
    {/if}
  </span>
</div>
{#if problem || inactive}
  <p class="problem" id="shortcut-problem" role="alert">
    {problem ?? t("settings.shortcut.inactive")}
  </p>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .sub {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .controls {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .recorder {
    min-width: 7em;
    font-variant-numeric: tabular-nums;
  }

  .recorder.recording {
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .problem {
    margin: 0;
    padding: 0 12px 8px;
    font-size: 0.85em;
    color: var(--warning);
  }
</style>
