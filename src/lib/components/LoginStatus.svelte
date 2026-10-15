<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ask } from "@tauri-apps/plugin-dialog";
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
  /** The browser couldn't be opened (Linux without `xdg-open`): the URL is spelled out. */
  let openFailed = $state(false);

  /** Signs in, or with `grant` adds gh-student's missing scopes to the active account. */
  async function signIn(grant = false) {
    if (!ghInstalled) return;
    pending = true;
    error = null;
    device = null;
    openFailed = false;
    try {
      const status = await (grant ? api.grantScopes() : api.login());
      onchange(status);
      await notifySignedIn(status);
    } catch (err) {
      if (errorKind(err) !== "cancelled") error = loginError(err);
    } finally {
      pending = false;
      device = null;
    }
  }

  function loginError(err: unknown): string {
    switch (errorKind(err)) {
      case "tokenFromEnv":
        return t("auth.envToken", { var: auth?.envToken ?? "GH_TOKEN" });
      case "timedOut":
        return t("auth.expired");
      default:
        return errorMessage(err);
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

  // gh-student can't accept or submit without these; a token from the environment can't
  // be widened from here.
  const needsScopes = $derived(
    Boolean(auth?.loggedIn && auth.missingScopes.length > 0 && !auth.envToken),
  );

  $effect(() => {
    session.signingIn = device !== null || needsScopes;
  });

  // gh just got installed (from the app or found by Check again): it may already hold an
  // account from an earlier install, so the status read while gh was missing is re-read.
  // Unset until the first run, which only records the starting value.
  let hadGh: boolean | null = null;
  $effect(() => {
    const installed = ghInstalled;
    if (installed && hadGh === false && auth && !auth.loggedIn) {
      void api
        .authStatus()
        .then((status) => {
          if (status.loggedIn) onchange(status);
        })
        .catch(() => {});
    }
    hadGh = installed;
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
    const envToken = auth?.envToken;
    // A token from the environment overrides gh's accounts: switching, adding or signing
    // out would change nothing the app uses.
    const items = await Promise.all(
      envToken
        ? [MenuItem.new({ text: t("auth.envTokenMenu", { var: envToken }), enabled: false })]
        : [
            ...accounts.map((account) =>
              CheckMenuItem.new({
                text: account.login,
                checked: account.active,
                action: () => void switchTo(account.login, account.active),
              }),
            ),
            PredefinedMenuItem.new({ item: "Separator" }),
            // Adding an account runs gh's device flow too.
            MenuItem.new({
              text: t(ghInstalled ? "auth.addAccount" : "auth.addAccountNeedsGh"),
              enabled: ghInstalled,
              action: () => void signIn(),
            }),
            ...(login
              ? [
                  MenuItem.new({
                    text: t("auth.signOut", { login }),
                    action: () => void signOut(login),
                  }),
                ]
              : []),
          ],
    );
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

  /** gh's sign-in is global: the terminal is signed out too, so it is confirmed first. */
  async function signOut(account: string) {
    error = null;
    const confirmed = await withDialog(() =>
      ask(t("auth.signOutConfirm", { login: account }), {
        title: t("auth.signOutTitle"),
        kind: "warning",
        okLabel: t("auth.signOutTitle"),
        cancelLabel: t("auth.cancel"),
      }),
    );
    if (!confirmed) return;
    try {
      onchange(await api.signOut(account));
    } catch (err) {
      error = loginError(err);
    }
  }

  /** Without a TTY gh never opens the browser itself, so the app does. */
  async function copyAndOpen() {
    if (!device) return;
    await navigator.clipboard.writeText(device.code).catch(() => {});
    try {
      await openUrl(device.url);
    } catch {
      openFailed = true;
    }
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
    <p class="url" class:error={openFailed}>
      {t(openFailed ? "auth.openFailed" : "auth.openAt", { url: device.url })}
    </p>
  </div>
{:else if auth?.loggedIn}
  <div class="signed-in">
    {#if needsScopes}
      <div class="row grant">
        <span>{t("auth.needsScopes")}</span>
        <button class="primary" disabled={pending} onclick={() => signIn(true)}>
          {t("auth.grant")}
        </button>
      </div>
    {/if}
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
  </div>
{:else if auth}
  <div class="row">
    <span>{ghInstalled ? t("auth.loggedOut") : t("auth.needsGh")}</span>
    <button class="primary" disabled={!ghInstalled || pending} onclick={() => signIn()}>
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

  .url {
    margin: 0;
    color: var(--fg-muted);
    -webkit-user-select: text;
    user-select: text;
  }

  .url.error {
    color: var(--danger);
  }

  /* The grant prompt and an error stack above and below the account pill. */
  .signed-in {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    min-width: 0;
  }

  .grant {
    align-self: stretch;
    margin-bottom: 6px;
  }
</style>
