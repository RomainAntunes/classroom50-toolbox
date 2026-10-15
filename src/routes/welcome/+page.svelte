<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api, type AuthStatus, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";
  import { chooseDefaultEditor, defaultEditor, editors, loadEditors } from "$lib/editor.svelte";
  import { pickCloneFolder } from "$lib/clone";
  import { loadSettings, saveSettings, settings } from "$lib/settings.svelte";
  import { session } from "$lib/session.svelte";
  import LoginStatus from "$lib/components/LoginStatus.svelte";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";

  // First run: what the app does, then the three things it needs. Every step can be
  // skipped; Settings has all of it later.
  const STEPS = ["welcome", "tools", "account", "folders"] as const;
  let step = $state(0);
  const current = $derived(settings.current);
  const editor = $derived(current ? defaultEditor() : null);
  const ghInstalled = $derived(session.tools.some((tool) => tool.tool === "gh" && tool.installed));
  const missing = $derived(session.tools.filter((tool) => !tool.installed).length);

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }

  async function signedIn(next: AuthStatus) {
    session.auth = next;
  }

  async function finish() {
    await saveSettings({ onboarded: true });
    await goto("/", { replaceState: true });
    // Signed in during the guide: start filling the list now.
    if (session.auth?.loggedIn) session.snapshot = await api.refreshProjects();
  }

  onMount(() => {
    void loadSettings();
    void loadEditors(true);
    void Promise.all([api.toolStatuses(), api.authStatus()]).then(([tools, auth]) => {
      session.tools = tools;
      session.auth = auth;
    });
  });
</script>

<header class="titlebar">
  <h1>{t("welcome.title")}</h1>
  <button class="skip" onclick={finish}>{t("welcome.skip")}</button>
</header>

<main>
  {#if STEPS[step] === "welcome"}
    <section class="intro">
      <p class="lead">{t("welcome.lead")}</p>
      <ul>
        <li>{t("welcome.point.list")}</li>
        <li>{t("welcome.point.actions")}</li>
        <li>{t("welcome.point.notify")}</li>
      </ul>
    </section>
  {:else if STEPS[step] === "tools"}
    <Section title={t("welcome.tools.title")}>
      <p class="hint">
        {missing ? t("welcome.tools.missing") : t("welcome.tools.ready")}
      </p>
      {#each session.tools as status (status.tool)}
        <ToolRow {status} onchange={replaceTool} />
      {/each}
    </Section>
  {:else if STEPS[step] === "account"}
    <Section title={t("welcome.account.title")}>
      <p class="hint">{t("welcome.account.hint")}</p>
      <div class="login">
        <LoginStatus auth={session.auth} {ghInstalled} onchange={signedIn} />
      </div>
    </Section>
  {:else if current}
    <Section title={t("welcome.folders.title")}>
      <div class="row">
        <span class="text">
          <span>{t("settings.folders.cloneInto")}</span>
          <span class="sub path" title={current.cloneFolder ?? undefined}
            >{current.cloneFolder ?? t("welcome.folders.cloneUnset")}</span
          >
        </span>
        <button onclick={pickCloneFolder}>
          {t(current.cloneFolder ? "settings.folders.change" : "welcome.folders.choose")}
        </button>
      </div>
      <div class="row">
        <span class="text">
          <span>{t("settings.editor.label")}</span>
          <span class="sub"
            >{t(editors.installed.length ? "settings.editor.hint" : "settings.editor.none")}</span
          >
        </span>
        {#if editor}
          <button onclick={chooseDefaultEditor}>{editor.name}</button>
        {/if}
      </div>
      <Toggle
        label={t("settings.folders.editors")}
        hint={t("settings.folders.editorsHint")}
        checked={current.scanEditorProjects}
        onchange={(on) => saveSettings({ scanEditorProjects: on })}
      />
    </Section>
  {/if}
</main>

<footer>
  <div class="dots" aria-hidden="true">
    {#each STEPS as name, i (name)}
      <span class:active={i === step}></span>
    {/each}
  </div>
  <div class="nav">
    {#if step > 0}
      <button onclick={() => (step -= 1)}>{t("welcome.back")}</button>
    {/if}
    {#if step < STEPS.length - 1}
      <button class="primary" onclick={() => (step += 1)}>
        {t(step === 0 ? "welcome.start" : "welcome.next")}
      </button>
    {:else}
      <button class="primary" onclick={finish}>{t("welcome.done")}</button>
    {/if}
  </div>
</footer>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 12px 14px 4px;
  }

  h1 {
    margin: 0;
    font-size: 1.1em;
    font-weight: 600;
  }

  .skip {
    background: transparent;
    color: var(--fg-muted);
  }

  main {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: var(--space) 12px;
  }

  .intro {
    padding: 4px 2px;
  }

  .lead {
    margin: 0 0 var(--space);
    font-size: 1.05em;
  }

  ul {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--fg-muted);
  }

  .hint {
    margin: 0;
    padding: 12px;
    color: var(--fg-muted);
  }

  .login {
    padding: 4px 12px 12px;
  }

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

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 10px 12px;
    border-top: 1px solid var(--border);
  }

  .dots {
    display: flex;
    gap: 6px;
  }

  .dots span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--border);
  }

  .dots span.active {
    background: var(--accent);
  }

  .nav {
    display: flex;
    gap: 6px;
  }
</style>
