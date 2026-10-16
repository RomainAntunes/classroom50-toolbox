<script lang="ts">
  import logo from "$lib/assets/logo.png";
  import { onMount, tick } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { changelogs, notesFor, versions } from "$lib/changelog";
  import { compareVersions, KINDS, type Entry, type EntryKind } from "$lib/changelog/versions.js";
  import { currentLocale, formatDay, t } from "$lib/i18n";
  import { loadSettings, saveSettings, settings } from "$lib/settings.svelte";
  import { appVersion, pendingNotes, seenVersion } from "$lib/whatsnew";

  // After an update, the versions not read yet. `?from=settings` opens it again from
  // Settings with the running version's notes, `&all` (the dev preview) with every version's.
  const from = page.url.searchParams.get("from");
  const all = page.url.searchParams.has("all");
  let shown = $state<string[] | null>(null);
  let heading = $state<HTMLHeadingElement | null>(null);
  const many = $derived((shown?.length ?? 0) > 1);

  interface Group {
    kind: EntryKind | null;
    entries: Entry[];
  }

  /** The entries in the UI's language (else English): kindless first, then by kind. */
  function groups(version: string): Group[] {
    const entries = notesFor(version, currentLocale());
    return [
      { kind: null, entries: entries.filter((entry) => !entry.kind) },
      ...KINDS.map((kind) => ({ kind, entries: entries.filter((entry) => entry.kind === kind) })),
    ].filter((group) => group.entries.length);
  }

  /** "Released 16 October 2026": the file's date read as a local day. */
  function released(version: string): string | null {
    const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(changelogs[version]?.date ?? "");
    if (!match) return null;
    const day = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
    return t("whatsNew.released", { date: formatDay(day) });
  }

  async function done() {
    if (!from) await saveSettings({ lastSeenVersion: await seenVersion(settings.current) });
    await goto(from === "settings" ? "/settings" : "/", { replaceState: true });
  }

  onMount(() => {
    void (async () => {
      const [loaded, current] = await Promise.all([loadSettings(), appVersion()]);
      const known = versions();
      if (all) shown = known;
      else if (from) shown = known.filter((v) => compareVersions(v, current) <= 0).slice(0, 1);
      else shown = await pendingNotes(loaded);
      // Screen readers start at the title; Tab then reaches Got it.
      await tick();
      heading?.focus();
    })();
  });
</script>

<main>
  {#if shown}
    <section class="hero">
      <img class="hero-logo" src={logo} alt="" />
      <h1 class="hero-title" tabindex="-1" bind:this={heading}>
        {shown.length === 1
          ? t("whatsNew.titleVersion", { version: shown[0] })
          : t("whatsNew.title")}
      </h1>
      {#if !from}
        <p class="lead">{many ? t("whatsNew.leadMany") : t("whatsNew.lead")}</p>
      {/if}
      {#if shown.length === 1 && released(shown[0])}
        <p class="date">{released(shown[0])}</p>
      {/if}
    </section>

    {#each shown as version (version)}
      <section class="version">
        {#if many}
          <div class="version-title">
            <h2>{t("whatsNew.version", { version })}</h2>
            {#if released(version)}<span class="date">{released(version)}</span>{/if}
          </div>
        {/if}
        {#each groups(version) as group (group.kind)}
          {#if group.kind}
            <svelte:element this={many ? "h3" : "h2"} class="kind"
              >{t(`whatsNew.kind.${group.kind}`)}</svelte:element
            >
          {/if}
          <!-- An explicit role: WebKit drops the list semantics of a list without bullets. -->
          <!-- svelte-ignore a11y_no_redundant_roles -->
          <ul class="points" role="list">
            {#each group.entries as entry, i (i)}
              <li>
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  {#if group.kind === "new"}
                    <path
                      d="M9.94 15.5a2 2 0 0 0-1.44-1.44l-6.13-1.58a.5.5 0 0 1 0-.96L8.5 9.94A2 2 0 0 0 9.94 8.5l1.58-6.14a.5.5 0 0 1 .96 0l1.58 6.14a2 2 0 0 0 1.44 1.44l6.14 1.58a.5.5 0 0 1 0 .96l-6.14 1.58a2 2 0 0 0-1.44 1.44l-1.58 6.14a.5.5 0 0 1-.96 0z"
                    />
                  {:else if group.kind === "improved"}
                    <path d="M22 7l-8.5 8.5-5-5L2 17M16 7h6v6" />
                  {:else if group.kind === "fixed"}
                    <path
                      d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"
                    />
                  {:else}
                    <path d="M20 6 9 17l-5-5" />
                  {/if}
                </svg>
                <span>{entry.text}</span>
              </li>
            {/each}
          </ul>
        {/each}
      </section>
    {:else}
      <p class="lead empty">{t("whatsNew.empty")}</p>
    {/each}
  {/if}
</main>

<footer>
  <button class="primary" onclick={done}>{t("whatsNew.done")}</button>
</footer>

<style>
  main {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 2) 12px var(--space);
  }

  h1,
  h2 {
    margin: 0;
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
    font-weight: 600;
  }

  /* Focused from script so screen readers start here; it isn't a control. */
  .hero-title:focus {
    outline: none;
  }

  .lead {
    margin: 0;
    color: var(--fg-muted);
  }

  .empty {
    text-align: center;
  }

  .date {
    margin: 0;
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .version {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 0.75);
  }

  .version-title {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space);
    padding: 0 4px;
  }

  .version-title h2 {
    font-size: 1em;
    font-weight: 600;
  }

  /* Like a settings section's title. */
  .kind {
    margin: calc(var(--space) * 0.5) 0 0;
    padding: 0 4px;
    font-size: 0.85em;
    font-weight: 600;
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

  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space);
    padding: 10px 12px;
    border-top: 1px solid var(--border);
  }
</style>
