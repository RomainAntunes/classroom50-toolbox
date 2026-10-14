<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
  } from "@tauri-apps/plugin-notification";
  import {
    api,
    DEVICE_CODE_EVENT,
    LOGIN_ENDED_EVENT,
    errorKind,
    errorMessage,
    withDialog,
    type AuthStatus,
    type DeviceCode,
  } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import { t } from "$lib/i18n";

  let {
    auth,
    ghInstalled,
    onchange,
  }: { auth: AuthStatus | null; ghInstalled: boolean; onchange: (a: AuthStatus) => void } =
    $props();

  let pending = $state(false);
  let device = $state<DeviceCode | null>(null);
  let error = $state<string | null>(null);

  async function signIn() {
    pending = true;
    error = null;
    device = null;
    try {
      const status = await api.login();
      onchange(status);
      await notifySignedIn(status);
    } catch (err) {
      if (errorKind(err) !== "cancelled") error = errorMessage(err);
    } finally {
      pending = false;
      device = null;
    }
  }

  // The browser takes focus during the device flow, which hides the popover, so the result
  // is reported through a system notification.
  async function notifySignedIn(status: AuthStatus) {
    if (!status.loggedIn) return;
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) {
      sendNotification({
        title: t("auth.notification.title"),
        body: t("auth.notification.body", { login: status.login ?? "" }),
      });
    }
  }

  $effect(() => {
    session.signingIn = device !== null;
  });

  const login = $derived(auth?.loggedIn ? auth.login : null);
  const avatar = $derived(login ? session.avatars[login] : undefined);

  $effect(() => {
    const current = login;
    if (current && !session.avatars[current]) {
      api
        .accountAvatar(current)
        .then((url) => {
          session.avatars[current] = url;
        })
        .catch(() => {});
    }
  });

  /** Native menu (arrow keys and Enter work): the accounts, then "Add another account". */
  async function showAccounts(event: MouseEvent) {
    event.preventDefault();
    const accounts = await api.listAccounts().catch(() => []);
    const items = await Promise.all([
      ...accounts.map((account) =>
        CheckMenuItem.new({
          text: account.login,
          checked: account.active,
          action: () => void switchTo(account.login, account.active),
        }),
      ),
      PredefinedMenuItem.new({ item: "Separator" }),
      MenuItem.new({ text: t("auth.addAccount"), action: () => void signIn() }),
    ]);
    const menu = await Menu.new({ items });
    await withDialog(() => menu.popup());
  }

  async function switchTo(next: string, active: boolean) {
    if (active) return;
    error = null;
    try {
      onchange(await api.switchAccount(next));
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function copyAndOpen() {
    if (!device) return;
    await navigator.clipboard.writeText(device.code).catch(() => {});
    await openUrl(device.url);
  }

  /** Hides the code at once; Rust kills gh and `login-ended` follows. */
  function cancel() {
    device = null;
    pending = false;
    void api.cancelLogin();
  }

  onMount(() => {
    const unlisteners = [
      listen<DeviceCode>(DEVICE_CODE_EVENT, (event) => {
        device = event.payload;
      }),
      // Also resets a view that didn't start the sign-in (e.g. after a reload).
      listen(LOGIN_ENDED_EVENT, () => {
        device = null;
        pending = false;
      }),
    ];
    return () => {
      for (const unlisten of unlisteners) void unlisten.then((stop) => stop());
    };
  });
</script>

{#if device}
  <div class="device">
    <span>{t("auth.waiting")}</span>
    <code>{device.code}</code>
    <div class="actions">
      <button class="primary" onclick={copyAndOpen}>{t("auth.copyAndOpen")}</button>
      <button onclick={cancel}>{t("auth.cancel")}</button>
    </div>
  </div>
{:else if auth?.loggedIn}
  <button
    class="account"
    title={t("auth.switchAccount")}
    aria-label={t("auth.loggedInAs", { login: auth.login ?? "" })}
    aria-haspopup="menu"
    onclick={showAccounts}
    oncontextmenu={showAccounts}
  >
    {#if avatar}
      <img src={avatar} alt="" />
    {:else}
      <span class="placeholder" aria-hidden="true"></span>
    {/if}
    <span class="login">{auth.login}</span>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>
  {#if error}
    <p class="error">{error}</p>
  {/if}
{:else if auth}
  <div class="row">
    <span>{ghInstalled ? t("auth.loggedOut") : t("auth.needsGh")}</span>
    <button class="primary" disabled={!ghInstalled || pending} onclick={signIn}>
      {t("auth.signIn")}
    </button>
  </div>
  {#if error}
    <p class="error">{error}</p>
  {/if}
{/if}

<style>
  .row,
  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
  }

  /* Quiet until hovered, like a toolbar item. */
  /* A pill as tall as the titlebar controls; the avatar sits 4px inside its round end. */
  .account {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    height: 26px;
    max-width: 100%;
    padding: 0 8px 0 4px;
    border-radius: 13px;
    background: transparent;
    color: inherit;
  }

  .account img,
  .placeholder {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--bg-control);
  }

  .login {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    /* Trim the line box to cap height and baseline, so centering it centers the letters
       on the avatar instead of leaving descender space below them. */
    text-box: trim-both cap alphabetic;
    /* Equal room above and below, so descenders (g, p, y) aren't clipped by the
       ellipsis overflow while the center stays put. */
    padding: 4px 0;
  }

  .account svg {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .device {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  code {
    font-family: ui-monospace, "SF Mono", "Cascadia Mono", "DejaVu Sans Mono", monospace;
    font-size: 1.6em;
    letter-spacing: 0.08em;
    color: var(--fg);
    -webkit-user-select: all;
    user-select: all;
  }

  .error {
    margin: 4px 0 0;
    color: var(--danger);
  }
</style>
