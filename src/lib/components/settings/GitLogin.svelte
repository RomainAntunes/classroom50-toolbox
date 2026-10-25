<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { api, errorKind, errorMessage, withDialog, type GitCredentials } from "#lib/api.js";
  import { formatList, t } from "#lib/i18n/index.js";
  import { announce } from "#lib/announce.svelte.js";
  import { atLeast } from "#lib/busy.js";
  import { session } from "#lib/session.svelte.js";
  import Spinner from "#lib/components/Spinner.svelte";

  /** Whether git signs in to github.com with gh outside the app (a terminal, an editor):
   * `gh auth setup-git`, offered once git, gh and a sign-in are there. */
  let credentials = $state<GitCredentials | null>(null);
  let busy = $state(false);

  const ready = $derived(
    session.tools.some((s) => s.tool === "git" && s.installed) &&
      session.tools.some((s) => s.tool === "gh" && s.installed) &&
      !!session.auth?.loggedIn,
  );

  // Read again when the tools or the account change (Check again, an install, a sign-in).
  $effect(() => {
    if (!ready) return;
    void session.auth?.login;
    void session.tools;
    api.gitCredentials().then(
      (next) => (credentials = next),
      () => (credentials = null),
    );
  });

  async function setUp() {
    if (busy || !credentials) return;
    let text = t("settings.git.confirm");
    if (credentials.others.length > 0) {
      text += "\n\n" + t("settings.git.replaces", { helpers: formatList(credentials.others) });
    }
    const go = await withDialog(() =>
      ask(text, {
        kind: "info",
        okLabel: t("settings.git.setUp"),
        cancelLabel: t("setup.cancel"),
      }),
    );
    if (!go) return;
    busy = true;
    try {
      credentials = await atLeast(api.setUpGitCredentials());
      announce(t("a11y.gitSetUp"));
    } catch (err) {
      const error = errorKind(err) === "notSignedIn" ? t("setup.signInFirst") : errorMessage(err);
      await withDialog(() => message(t("settings.git.failed", { error }), { kind: "error" }));
    } finally {
      busy = false;
    }
  }
</script>

{#if ready && credentials}
  <div class="git">
    <div class="row">
      <span class="dot" class:ok={credentials.usesGh} aria-hidden="true"></span>
      <span class="name"
        >{t("settings.git.title")}<span class="sr-only"
          >, {t(credentials.usesGh ? "settings.git.on" : "settings.git.off")}</span
        ></span
      >
      {#if credentials.usesGh}
        <span class="meta" aria-hidden="true">{t("settings.git.on")}</span>
      {:else}
        <button class="primary" disabled={busy} aria-busy={busy} onclick={setUp}
          >{#if busy}<Spinner />{/if}{t("settings.git.setUp")}</button
        >
      {/if}
    </div>
    <p class="sub">{t(credentials.usesGh ? "settings.git.onHint" : "settings.git.offHint")}</p>
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space);
    padding: 8px 12px;
  }

  .name {
    flex: 1;
  }

  .meta {
    color: var(--fg-muted);
  }

  /* Under the name, past the dot. */
  .sub {
    margin: -4px 0 0;
    padding-block: 0 8px;
    padding-inline: calc(12px + 8px + var(--space)) 12px;
    color: var(--fg-muted);
    font-size: 0.85em;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warning);
  }

  .dot.ok {
    background: var(--success);
  }
</style>
