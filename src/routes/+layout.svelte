<script lang="ts">
  import "$lib/theme/theme.css";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    DEEP_LINK_EVENT,
    NOTIFY_EVENT,
    POPOVER_HIDDEN_EVENT,
    UNSEEN_EVENT,
    type Notice,
  } from "$lib/api";
  import { handleDeepLink } from "$lib/deeplink";
  import { currentLocale, setLocale, t } from "$lib/i18n";
  import { showNotice } from "$lib/notices";
  import { loadEditors } from "$lib/editor.svelte";
  import { initTheme } from "$lib/theme/preview.svelte";
  import Prompt from "$lib/components/Prompt.svelte";
  import { announcer } from "$lib/announce.svelte";
  import { prompt } from "$lib/prompt.svelte";
  import { session } from "$lib/session.svelte";
  import { loadSettings, settings } from "$lib/settings.svelte";

  let { children } = $props();

  // The webview's own menu (Reload, Back, Inspect Element) makes no sense in a tray app.
  // Text fields keep theirs for copy and paste.
  function onContextMenu(event: MouseEvent) {
    const target = event.target as HTMLElement | null;
    if (!target?.closest("input, textarea, [contenteditable]")) {
      event.preventDefault();
    }
  }

  // Escape closes what is open, innermost first: the prompt and a row's details handle it
  // (and mark it handled); with nothing left to close it hides the popover, like a click
  // outside. A text field with text clears it first; a switch or checkbox (whose `value`
  // is "on") doesn't count.
  const TEXT_INPUTS = ["text", "search", "email", "url", "tel", "password", "number"];
  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== "Escape" || event.defaultPrevented || prompt.current) return;
    const target = event.target;
    if (target instanceof HTMLInputElement && TEXT_INPUTS.includes(target.type) && target.value) {
      return;
    }
    event.preventDefault();
    void api.hidePopover();
  }

  // The `language` setting, live: `t` is reactive, so the whole UI follows at once.
  // "System" reads the OS's languages from Rust: WKWebView's `navigator.languages` holds
  // the app bundle's localizations, not the user's. They stand in until Rust answers.
  const language = $derived(settings.current?.language ?? "system");
  let systemLanguages = $state<readonly string[]>(navigator.languages);
  $effect(() => {
    setLocale(language, systemLanguages);
    document.documentElement.lang = currentLocale();
  });

  // Rust writes calendar notes and the tray menu with these words (placeholders filled
  // there); sent again when the language changes.
  $effect(() => {
    void api.setCalendarLabels({
      class: t("calendar.notes.class"),
      group: t("calendar.notes.group"),
      groupNumber: t("calendar.notes.groupNumber"),
      graded: t("calendar.notes.graded"),
      submitted: t("calendar.notes.submitted"),
      notAccepted: t("calendar.notes.notAccepted"),
    });
    void api.setTrayLabels({ open: t("tray.open"), quit: t("tray.quit") });
  });

  // Lives in the layout so notices arrive whichever page is shown, and while hidden.
  onMount(() => {
    void loadSettings();
    void api.systemLanguages().then((languages) => {
      if (languages.length) systemLanguages = languages;
    });
    void loadEditors();
    void api.appInfo().then((info) => initTheme(info.os));
    void api.unseen().then((ids) => {
      session.unseen = ids;
    });
    const unlisteners = [
      listen<Notice>(NOTIFY_EVENT, (event) => void showNotice(event.payload)),
      listen<string[]>(UNSEEN_EVENT, (event) => {
        session.unseen = event.payload;
      }),
      listen(DEEP_LINK_EVENT, () => void handleDeepLink()),
      listen(POPOVER_HIDDEN_EVENT, () => {
        session.focus = null;
      }),
    ];
    // A link that started the app came before this page could hear about it.
    void handleDeepLink();
    return () => {
      for (const unlisten of unlisteners) void unlisten.then((stop) => stop());
    };
  });
</script>

<svelte:document oncontextmenu={onContextMenu} onkeydown={onKeyDown} />

<div class="popover">
  {@render children()}
  <Prompt />
  <div class="sr-only" role="status">{announcer.text}</div>
</div>

<style>
  .popover {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-window);
    border-radius: var(--radius-window);
    overflow: hidden;
  }

  :global(:root[data-os="linux"]) .popover {
    border: 1px solid var(--border);
  }
</style>
