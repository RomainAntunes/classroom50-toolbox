<script lang="ts">
  import "#lib/theme/theme.css";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import {
    api,
    onEvent,
    DEEP_LINK_EVENT,
    NOTIFY_EVENT,
    POPOVER_HIDDEN_EVENT,
    SETTINGS_UPDATED_EVENT,
    UNSEEN_EVENT,
    UPDATE_STATE_EVENT,
    type Settings,
    type UpdateStatus,
  } from "#lib/api.js";
  import { handleDeepLink, linkActive } from "#lib/deeplink.js";
  import { currentDirection, currentLocale, setLocale, t } from "#lib/i18n/index.js";
  import { showNotice } from "#lib/notices.js";
  import { loadEditors } from "#lib/editor.svelte.js";
  import { initTheme } from "#lib/theme/preview.svelte.js";
  import Prompt from "#lib/components/Prompt.svelte";
  import { announce, announcer } from "#lib/announce.svelte.js";
  import { prompt } from "#lib/prompt.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { loadSettings, settings } from "#lib/settings.svelte.js";
  import { pendingNotes } from "#lib/whatsnew.js";

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

  // The `language` setting, live: `t` is reactive, so the whole UI follows at once, and
  // `dir` lays it out right to left for Arabic, Hebrew... (components use logical CSS
  // properties only, so they follow). "System" reads the OS's languages from Rust:
  // WKWebView's `navigator.languages` holds the app bundle's localizations, not the
  // user's. They stand in until Rust answers.
  const language = $derived(settings.current?.language ?? "system");
  let systemLanguages = $state<readonly string[]>(navigator.languages);
  $effect(() => {
    setLocale(language, systemLanguages);
    document.documentElement.lang = currentLocale();
    document.documentElement.dir = currentDirection();
  });

  // Rust writes calendar notes and the tray menu with these words (placeholders filled
  // there; names pass through `t` as themselves, so they are isolated in a right-to-left
  // language, numbers stay bare as `fill` leaves them); sent again when the language
  // changes.
  $effect(() => {
    void api.setCalendarLabels({
      class: t("calendar.notes.class", { name: "{name}" }),
      group: t("calendar.notes.group", { name: "{name}" }),
      groupNumber: t("calendar.notes.groupNumber"),
      graded: t("calendar.notes.graded"),
      submitted: t("calendar.notes.submitted"),
      notAccepted: t("calendar.notes.notAccepted"),
    });
    void api.setTrayLabels({ open: t("tray.open"), quit: t("tray.quit") });
  });

  // Self-update, for the main page's banner and Settings > About. A screen reader hears
  // when a download finishes (the system notification comes from Rust's `notify`).
  function setUpdate(next: UpdateStatus) {
    const before = session.update?.state.phase;
    session.update = next;
    if (next.state.phase === "ready" && before && before !== "ready") {
      announce(t("update.readyAnnounce", { version: next.state.version }));
    }
  }

  /**
   * Where the popover reopens once hidden. Like JetBrains Toolbox, on the list; on What's
   * new while its notes are unread (a link may have taken its place). The first-run guide
   * and What's new after an update keep their place, and a pinned popover (a dialog or
   * menu up) never hides.
   */
  async function reopenOn() {
    const { pathname, searchParams } = page.url;
    const again = pathname === "/whats-new" && searchParams.has("from");
    if (pathname === "/welcome" || (pathname === "/whats-new" && !again)) return;
    const current = settings.current;
    if (current && !linkActive() && (await pendingNotes(current)).length) {
      await goto("/whats-new", { replace: true });
    } else if (pathname.startsWith("/settings") || again) {
      await goto("/", { replace: true });
    }
  }

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
    // A popover opened mid-download shows where it stands.
    void api.updateStatus().then(setUpdate);
    const unlisteners = [
      onEvent(NOTIFY_EVENT, (notice) => void showNotice(notice)),
      onEvent(UNSEEN_EVENT, (ids) => {
        session.unseen = ids;
      }),
      onEvent(SETTINGS_UPDATED_EVENT, (saved) => {
        settings.current = saved;
      }),
      onEvent(UPDATE_STATE_EVENT, (status) => setUpdate(status)),
      onEvent(DEEP_LINK_EVENT, () => void handleDeepLink()),
      onEvent(POPOVER_HIDDEN_EVENT, () => {
        session.focus = null;
        void reopenOn();
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
