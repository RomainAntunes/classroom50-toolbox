<script lang="ts">
  import logo from "$lib/assets/logo.png";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api, type AuthStatus, type ToolStatus } from "$lib/api";
  import { t } from "$lib/i18n";
  import { announce } from "$lib/announce.svelte";
  import { chooseDefaultEditor, defaultEditor, editors, loadEditors } from "$lib/editor.svelte";
  import { pickCloneFolder } from "$lib/clone";
  import { loadSettings, saveSettings, settings } from "$lib/settings.svelte";
  import { session } from "$lib/session.svelte";
  import { seenVersion } from "$lib/whatsnew";
  import LoginStatus from "$lib/components/LoginStatus.svelte";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import ToolRow from "$lib/components/ToolRow.svelte";

  // First run: what the app does, then the three things it needs. Every step can be
  // skipped; Settings has all of it later.
  const STEPS = ["welcome", "tools", "account", "folders"] as const;
  let step = $state(0);
  let primary = $state<HTMLButtonElement | null>(null);
  const current = $derived(settings.current);
  const editor = $derived(current ? defaultEditor() : null);
  const ghInstalled = $derived(session.tools.some((tool) => tool.tool === "gh" && tool.installed));
  const missing = $derived(session.tools.filter((tool) => !tool.installed || tool.outdated).length);

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }

  async function signedIn(next: AuthStatus) {
    session.auth = next;
  }

  /** Moves to another step and says so, since the focus stays on the buttons. */
  function go(next: number) {
    step = next;
    announce(t("welcome.step", { step: next + 1, total: STEPS.length }));
    // Back vanishes on the first step; the main button takes the focus it had.
    if (next === 0) primary?.focus();
  }

  async function finish() {
    // A first run starts with this version: its notes are no news.
    await saveSettings({ onboarded: true, lastSeenVersion: await seenVersion(settings.current) });
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
  <!-- The first step's title is the page heading; later steps keep it up here. -->
  {#if step > 0}<h1>{t("welcome.title")}</h1>{:else}<span></span>{/if}
  <button class="skip" onclick={finish}>{t("welcome.skip")}</button>
</header>

<main>
  {#if STEPS[step] === "welcome"}
    <section class="hero">
      <img class="hero-logo" src={logo} alt="" />
      <h1 class="hero-title">{t("welcome.title")}</h1>
      <p class="lead">{t("welcome.lead")}</p>
    </section>
    <ul class="points">
      <li>
        <svg class="mirror-rtl" viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01" /></svg
        >
        <span>{t("welcome.point.list")}</span>
      </li>
      <li>
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M12 3v12m0 0-4-4m4 4 4-4M5 21h14" /></svg
        >
        <span>{t("welcome.point.actions")}</span>
      </li>
      <li>
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0" /></svg
        >
        <span>{t("welcome.point.notify")}</span>
      </li>
    </ul>
  {:else if STEPS[step] === "tools"}
    <p class="hint">
      {missing ? t("welcome.tools.missing") : t("welcome.tools.ready")}
    </p>
    <Section title={t("welcome.tools.title")}>
      {#each session.tools as status (status.tool)}
        <ToolRow {status} onchange={replaceTool} />
      {/each}
    </Section>
  {:else if STEPS[step] === "account"}
    <p class="hint">
      {ghInstalled ? t("welcome.account.hint") : t("welcome.account.needsGh")}
    </p>
    <Section title={t("welcome.account.title")}>
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
            >{#if current.cloneFolder}<bdi dir="ltr">{current.cloneFolder}</bdi>{:else}<bdi
                >{t("welcome.folders.cloneUnset")}</bdi
              >{/if}</span
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
          <button
            aria-haspopup="menu"
            aria-label={`${t("settings.editor.label")}: ${editor.name}`}
            onclick={(event) => chooseDefaultEditor(event.currentTarget)}>{editor.name}</button
          >
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
  <!-- One button for Get started, Next and Done, so the focus stays on it. -->
  <div class="nav">
    {#if step > 0}
      <button onclick={() => go(step - 1)}>{t("welcome.back")}</button>
    {/if}
    <button
      class="primary"
      bind:this={primary}
      onclick={() => (step < STEPS.length - 1 ? go(step + 1) : finish())}
    >
      {t(step === 0 ? "welcome.start" : step < STEPS.length - 1 ? "welcome.next" : "welcome.done")}
    </button>
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

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 12px 0;
    text-align: center;
  }

  .hero-logo {
    width: 64px;
    height: 64px;
    margin-bottom: 4px;
  }

  .hero-title {
    font-size: 1.2em;
  }

  .lead {
    margin: 0;
    color: var(--fg-muted);
  }

  .points {
    margin: 0;
    padding: 4px 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
  }

  .points li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 12px;
  }

  .points svg {
    flex-shrink: 0;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .hint {
    margin: 0 4px calc(var(--space) * -1);
    color: var(--fg-muted);
    font-size: 0.9em;
  }

  /* Same inset on every side, like the other rows. */
  .login {
    padding: 10px 12px;
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

  /* A path cut short at its start (`direction: rtl` puts the ellipsis there), the path
     itself kept left to right by its `<bdi dir="ltr">` so its leading `/` stays put. */
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  /* `direction: rtl` above only puts the ellipsis at the path's start; the text still lines
     up with the UI's start edge. */
  :global(:root[dir="rtl"]) .path {
    text-align: right;
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
