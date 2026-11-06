<script lang="ts">
  import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, withDialog, type AuthStatus } from "#lib/api.js";
  import { cancelLogin, copyAndOpen, grant, login, loginError, signIn } from "#lib/login.svelte.js";
  import { popupMenu } from "#lib/keyboard.js";
  import { session } from "#lib/session.svelte.js";
  import { t } from "#lib/i18n/index.js";
  import Spinner from "./Spinner.svelte";

  // The sign-in itself lives in `login.svelte.ts` (it outlives this view: the browser hides
  // the popover, and Settings runs a grant too); this renders it and the account menu.
  let {
    auth,
    ghInstalled,
    onchange,
  }: { auth: AuthStatus | null; ghInstalled: boolean; onchange: (a: AuthStatus) => void } =
    $props();

  const pending = $derived(login.pending);
  const device = $derived(login.device);
  // Settings' Set up SSH shows its own error there.
  const error = $derived(login.error?.from === "footer" ? login.error.text : null);

  function startSignIn() {
    if (ghInstalled) void signIn(onchange);
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

  const account = $derived(auth?.loggedIn ? auth.login : null);
  const avatar = $derived(account ? session.avatars[account] : undefined);

  $effect(() => {
    const current = account;
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
    const anchor = event.currentTarget as HTMLElement;
    const accounts = await api.listAccounts().catch(() => []);
    const envToken = auth?.envToken;
    // A token from the environment overrides gh's accounts: switching, adding or signing
    // out would change nothing the app uses.
    const items = await Promise.all(
      envToken
        ? [MenuItem.new({ text: t("auth.envTokenMenu", { var: envToken }), enabled: false })]
        : [
            ...accounts.map((other) =>
              CheckMenuItem.new({
                text: other.login,
                checked: other.active,
                action: () => void switchTo(other.login, other.active),
              }),
            ),
            PredefinedMenuItem.new({ item: "Separator" }),
            // Adding an account runs gh's device flow too, after the same question.
            MenuItem.new({
              text: t(ghInstalled ? "auth.addAccount" : "auth.addAccountNeedsGh"),
              enabled: ghInstalled && !pending,
              action: startSignIn,
            }),
            ...(account
              ? [
                  MenuItem.new({
                    text: t("auth.signOut", { login: account }),
                    action: () => void signOut(account),
                  }),
                ]
              : []),
          ],
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  async function switchTo(next: string, active: boolean) {
    if (active) return;
    login.error = null;
    try {
      onchange(await api.switchAccount(next));
    } catch (err) {
      login.error = { text: loginError(err), from: "footer" };
    }
  }

  /** gh's sign-in is global: the terminal is signed out too, so it is confirmed first. */
  async function signOut(name: string) {
    login.error = null;
    const confirmed = await withDialog(() =>
      ask(t("auth.signOutConfirm", { login: name }), {
        title: t("auth.signOutTitle"),
        kind: "warning",
        okLabel: t("auth.signOutTitle"),
        cancelLabel: t("auth.cancel"),
      }),
    );
    if (!confirmed) return;
    try {
      onchange(await api.signOut(name));
    } catch (err) {
      login.error = { text: loginError(err), from: "footer" };
    }
  }
</script>

{#if device}
  <div class="device">
    <span>{t("auth.waiting")}</span>
    <code dir="ltr">{device.code}</code>
    <div class="actions">
      <button class="primary" onclick={copyAndOpen}>{t("auth.copyAndOpen")}</button>
      <button onclick={cancelLogin}>{t("auth.cancel")}</button>
    </div>
    <p class="url" class:error={login.openFailed}>
      {t(login.openFailed ? "auth.openFailed" : "auth.openAt", { url: device.url })}
    </p>
  </div>
{:else if auth?.loggedIn}
  <div class="signed-in">
    {#if needsScopes}
      <div class="row grant">
        <span>{t("auth.needsScopes")}</span>
        <button
          class="primary"
          disabled={pending}
          aria-busy={pending}
          onclick={() => grant([], onchange)}
        >
          {#if pending}<Spinner />{/if}{t("auth.grant")}
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
    {#if login.step}
      <p class="url">{t(`login.protocol.${login.step}`)}</p>
    {/if}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  </div>
{:else if auth}
  <div class="row">
    <span>{ghInstalled ? t("auth.loggedOut") : t("auth.needsGh")}</span>
    <button
      class="primary"
      disabled={!ghInstalled || pending}
      aria-busy={pending}
      onclick={startSignIn}
    >
      {#if pending}<Spinner />{/if}{t("auth.signIn")}
    </button>
  </div>
  {#if login.step}
    <p class="url">{t(`login.protocol.${login.step}`)}</p>
  {/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
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
    padding-block: 0;
    padding-inline: 4px 8px;
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
