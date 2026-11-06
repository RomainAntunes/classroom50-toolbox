<script lang="ts">
  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorMessage,
    withDialog,
    GIT_PROTOCOLS,
    type GitProtocol,
    type SshStatus,
  } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import { announce } from "#lib/announce.svelte.js";
  import { atLeast } from "#lib/busy.js";
  import { popupMenu } from "#lib/keyboard.js";
  import { rememberProtocol } from "#lib/links.js";
  import { cancelLogin, copyAndOpen, grant, login } from "#lib/login.svelte.js";
  import { scrollBehavior } from "#lib/motion.js";
  import { page } from "$app/state";
  import { session } from "#lib/session.svelte.js";
  import {
    SSH_GUIDE,
    SSH_KEYS,
    confirmHostKey,
    probe,
    protocolName,
    setUpKey,
    statusText,
  } from "#lib/ssh.js";
  import Spinner from "#lib/components/Spinner.svelte";

  /** How git reaches GitHub (gh's `git_protocol`, which gh in a terminal follows too) and
   *  whether SSH works, with the fix when there is one. Probed on its own only when SSH is in
   *  use (gh set to it, or a clone's origin); otherwise on a click. No timer. */
  let protocol = $state<GitProtocol | null>(null);
  let status = $state<SshStatus | null>(null);
  let checking = $state(false);
  let switching = $state(false);
  let acting = $state(false);

  const ready = $derived(
    session.tools.some((s) => s.tool === "gh" && s.installed) && !!session.auth?.loggedIn,
  );
  const usesSsh = $derived(
    protocol === "ssh" || !!session.snapshot?.projects.some((p) => p.local?.origin === "ssh"),
  );

  // Read again for another account (a switch or a sign-in while Settings shows).
  let loaded: string | null = null;
  $effect(() => {
    const account = session.auth?.login ?? null;
    if (!ready || loaded === account) return;
    loaded = account;
    status = null;
    void api.gitProtocol().then(
      (value) => {
        protocol = value;
        rememberProtocol(value);
        if (usesSsh) void check(false);
      },
      () => (protocol = "https"),
    );
  });

  async function check(fresh: boolean) {
    if (checking) return;
    checking = true;
    try {
      status = await atLeast(probe(fresh));
      announce(statusText(status));
    } finally {
      checking = false;
    }
  }

  async function chooseProtocol(anchor: HTMLElement) {
    const items = await Promise.all(
      GIT_PROTOCOLS.map((choice) =>
        CheckMenuItem.new({
          text: protocolName(choice),
          checked: choice === protocol,
          action: () => void pickProtocol(choice),
        }),
      ),
    );
    await popupMenu(await Menu.new({ items }), anchor);
  }

  async function pickProtocol(next: GitProtocol) {
    if (next === protocol || switching) return;
    const name = protocolName(next);
    const lines = [t("settings.ssh.confirm", { protocol: name })];
    // Known not to work yet: said before, not after.
    if (next === "ssh" && status && status.state !== "ready") lines.push(statusText(status));
    const go = await withDialog(() =>
      ask(lines.join("\n\n"), {
        kind: "info",
        okLabel: t("settings.ssh.use", { protocol: name }),
        cancelLabel: t("setup.cancel"),
      }),
    );
    if (!go) return;
    switching = true;
    try {
      protocol = await atLeast(api.setGitProtocol(next));
      rememberProtocol(protocol);
      announce(t("a11y.protocolSet", { protocol: protocolName(protocol) }));
    } catch (err) {
      await withDialog(() =>
        message(t("settings.ssh.failed", { error: errorMessage(err) }), { kind: "error" }),
      );
    } finally {
      switching = false;
    }
    if (protocol === "ssh" && !status) void check(false);
  }

  async function act(run: () => Promise<SshStatus | null>) {
    if (acting) return;
    acting = true;
    try {
      status = (await atLeast(run())) ?? status;
    } finally {
      acting = false;
    }
  }

  const fix = $derived(
    status?.state === "hostKeyUnknown"
      ? { label: t("ssh.trust"), run: () => confirmHostKey(false) }
      : status?.state === "hostKeyChanged"
        ? { label: t("ssh.replace"), run: () => confirmHostKey(true) }
        : status?.state === "noKey"
          ? { label: t("ssh.setUp"), run: () => setUpKey(grantScope) }
          : null,
  );

  // Set up SSH's grant: its code and any error show here (`from`), not only in the footer.
  const grantScope = () => grant(["writePublicKey"], undefined, "settings");
  const mine = $derived(login.from === "settings");
  const error = $derived(login.error?.from === "settings" ? login.error.text : null);

  // "SSH settings…" from a dialog lands here (`/settings#ssh`).
  let element: HTMLElement | undefined = $state();
  $effect(() => {
    if (!element || page.url.hash !== "#ssh") return;
    element.scrollIntoView({ block: "start", behavior: scrollBehavior() });
    element.focus({ preventScroll: true });
  });

  /** A link the OS couldn't open (no browser): spelled out instead. */
  let unopened = $state<string | null>(null);
  function open(url: string) {
    openUrl(url).catch(() => (unopened = url));
  }
</script>

{#if ready && protocol}
  <div class="ssh" id="ssh" tabindex="-1" bind:this={element}>
    <div class="row">
      <span class="text">
        <span>{t("settings.ssh.cloneWith")}</span>
        <span class="sub">{t("settings.ssh.cloneWithHint")}</span>
      </span>
      <button
        aria-haspopup="menu"
        aria-label={`${t("settings.ssh.cloneWith")}: ${protocolName(protocol)}`}
        disabled={switching}
        aria-busy={switching}
        onclick={(event) => chooseProtocol(event.currentTarget)}
        >{#if switching}<Spinner />{/if}{protocolName(protocol)}</button
      >
    </div>
    <div class="row">
      <span
        class="dot"
        class:ok={status?.state === "ready"}
        class:unknown={!status}
        aria-hidden="true"
      ></span>
      <span class="text">
        <span>{t("settings.ssh.title")}</span>
        <span class="sub"
          >{checking
            ? t("login.protocol.checking")
            : status
              ? statusText(status)
              : t("settings.ssh.unchecked")}</span
        >
      </span>
      {#if fix && !checking}
        <button class="primary" disabled={acting} aria-busy={acting} onclick={() => act(fix.run)}
          >{#if acting}<Spinner />{/if}{fix.label}</button
        >
      {/if}
      <!-- Fixed outside the app (a key added on GitHub, ssh-add): checked again from here. -->
      {#if status?.state !== "ready"}
        <button disabled={checking || acting} aria-busy={checking} onclick={() => check(true)}
          >{#if checking}<Spinner />{/if}{t(
            status ? "setup.recheck" : "settings.ssh.check",
          )}</button
        >
      {/if}
    </div>
    {#if login.device && mine}
      <!-- Set up SSH asked GitHub for the key scope: its code, here as in the footer. -->
      <div class="device">
        <span>{t("auth.waiting")}</span>
        <code dir="ltr">{login.device.code}</code>
        <div class="actions">
          <button class="primary" onclick={copyAndOpen}>{t("auth.copyAndOpen")}</button>
          <button onclick={cancelLogin}>{t("auth.cancel")}</button>
        </div>
        <p class="sub" class:error={login.openFailed}>
          {t(login.openFailed ? "auth.openFailed" : "auth.openAt", { url: login.device.url })}
        </p>
      </div>
    {:else if error}
      <p class="device error" role="alert">{error}</p>
    {/if}
    <p class="links">
      <button class="link" onclick={() => open(SSH_GUIDE)}>{t("settings.ssh.guide")}</button>
      <span aria-hidden="true">·</span>
      <button class="link" onclick={() => open(SSH_KEYS)}>{t("settings.ssh.keys")}</button>
    </p>
    {#if unopened}
      <p class="links url" dir="ltr">{unopened}</p>
    {/if}
  </div>
{/if}

<style>
  @import "./row.css";
  @import "./label.css";

  .text {
    flex: 1;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--warning);
  }

  .dot.ok {
    background: var(--success);
  }

  .dot.unknown {
    background: var(--fg-muted);
  }

  .device {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0 12px 8px;
  }

  .device .sub {
    margin: 0;
  }

  code {
    font-family: var(--font-mono);
    font-size: 1.6em;
    letter-spacing: 0.08em;
    -webkit-user-select: all;
    user-select: all;
  }

  .actions {
    display: flex;
    gap: var(--space);
  }

  .error {
    color: var(--danger);
  }

  /* Under the status, past the dot. */
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: -4px 0 0;
    padding-block: 0 8px;
    padding-inline: calc(12px + 8px + var(--space)) 12px;
    color: var(--fg-muted);
    font-size: 0.85em;
  }

  .url {
    -webkit-user-select: text;
    user-select: text;
  }

  .ssh:focus {
    outline: none;
  }

  .link {
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
</style>
