<script lang="ts">
  import "$lib/theme/theme.css";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
  } from "@tauri-apps/plugin-notification";
  import { api, NOTIFY_EVENT, UNSEEN_EVENT, type Notice } from "$lib/api";
  import { fromNow, setLocale, t } from "$lib/i18n";
  import { session } from "$lib/session.svelte";

  let { children } = $props();

  // The webview's own menu (Reload, Back, Inspect Element) makes no sense in a tray app.
  // Text fields keep theirs for copy and paste.
  function onContextMenu(event: MouseEvent) {
    const target = event.target as HTMLElement | null;
    if (!target?.closest("input, textarea, [contenteditable]")) {
      event.preventDefault();
    }
  }

  /** Words a notice from Rust (`alerts.rs`) and shows it as a system notification. */
  async function notify(notice: Notice) {
    const { kind, ...params } = notice;
    const values: Record<string, string | number> = { ...params };
    if (notice.kind === "due") values.when = fromNow(new Date(notice.due));
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) {
      sendNotification({
        title: t(`alerts.${kind}.title`, values),
        body: t(`alerts.${kind}.body`, values),
      });
    }
  }

  // Lives in the layout so notices arrive whichever page is shown, and while hidden.
  onMount(() => {
    setLocale(navigator.language);
    void api.appInfo().then((info) => (document.documentElement.dataset.os = info.os));
    void api.unseen().then((ids) => {
      session.unseen = ids;
    });
    const unlisteners = [
      listen<Notice>(NOTIFY_EVENT, (event) => void notify(event.payload)),
      listen<string[]>(UNSEEN_EVENT, (event) => {
        session.unseen = event.payload;
      }),
    ];
    return () => {
      for (const unlisten of unlisteners) void unlisten.then((stop) => stop());
    };
  });
</script>

<svelte:document oncontextmenu={onContextMenu} />

<div class="popover">
  {@render children()}
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
