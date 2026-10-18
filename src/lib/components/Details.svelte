<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { GradeTest, ReleaseNotes } from "#lib/api.js";
  import { secondsLabel, t } from "#lib/i18n/index.js";

  /** Failed tests listed before "and N more". */
  const MAX_TESTS = 8;

  /**
   * The expanded part of a row: the assignment's description, label/value lines, then the
   * tests the last grade failed, each opening on why (the autograder's output, led by a line
   * when it stopped the program at its time limit), and what else the release says.
   */
  let {
    description,
    items,
    failed = [],
    notes = null,
    gradeUrl = null,
  }: {
    description: string | null;
    items: [label: string, value: string][];
    failed?: GradeTest[];
    notes?: ReleaseNotes | null;
    /** The graded release, for output cut short here. */
    gradeUrl?: string | null;
  } = $props();

  // Which failed tests (by index) and whether the notes are open.
  let opened = $state<Record<number, boolean>>({});
  let notesOpen = $state(false);
  // Until the release body is read, only its page says why.
  const explained = $derived(
    failed.some((test) => test.output !== null || test.timedOutAfter !== null) || !!notes?.notes,
  );
</script>

<!-- Test output is code, left to right whatever the UI; the autograder's report follows its
     own words line by line. -->
{#snippet output(text: string, clipped: boolean, report = false, timedOut: number | null = null)}
  <div class="output">
    {#if timedOut !== null}
      <p class="timeout">{t("details.timedOut", { duration: secondsLabel(timedOut) })}</p>
      <p class="hint">{t("details.timedOutHint")}</p>
    {/if}
    {#if text}<pre class:report dir={report ? null : "ltr"}>{text}</pre>{/if}
    {#if clipped && gradeUrl}
      <button class="link" onclick={() => gradeUrl && openUrl(gradeUrl)}
        >{t("details.seeOnGitHub")}</button
      >
    {/if}
  </div>
{/snippet}

<div class="details">
  {#if description}
    <p class="description">{description}</p>
  {/if}
  <dl>
    {#each items as [label, value] (label)}
      <dt>{label}</dt>
      <dd title={value} class:lines={value.includes("\n")}><bdi>{value}</bdi></dd>
    {/each}
  </dl>
  {#if failed.length}
    <p class="heading">{t(explained ? "details.failedTests" : "details.failedTestsHint")}</p>
    <!-- An explicit role: WebKit drops the list semantics of a list without bullets. -->
    <!-- svelte-ignore a11y_no_redundant_roles -->
    <ul class="tests" role="list">
      {#each failed.slice(0, MAX_TESTS) as test, i (i)}
        <li>
          {#if test.output !== null || test.outputClipped || test.timedOutAfter !== null}
            <button
              class="test"
              aria-expanded={!!opened[i]}
              title={test.name}
              onclick={() => (opened[i] = !opened[i])}
            >
              <span class="chevron mirror-rtl" class:open={opened[i]} dir="ltr" aria-hidden="true"
                >›</span
              >
              <span class="name"><bdi>{test.name}</bdi></span>
              <span class="points">{test.score}/{test.maxScore}</span>
            </button>
            {#if opened[i]}
              {@render output(test.output ?? "", test.outputClipped, false, test.timedOutAfter)}
            {/if}
          {:else}
            <div class="test">
              {#if explained}<span class="chevron" aria-hidden="true"></span>{/if}
              <span class="name" title={test.name}><bdi>{test.name}</bdi></span>
              <span class="points">{test.score}/{test.maxScore}</span>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if failed.length > MAX_TESTS}
      <p class="more">{t("details.moreTests", { count: failed.length - MAX_TESTS })}</p>
    {/if}
    {#if notes?.notes}
      <button class="test" aria-expanded={notesOpen} onclick={() => (notesOpen = !notesOpen)}>
        <span class="chevron mirror-rtl" class:open={notesOpen} dir="ltr" aria-hidden="true">›</span
        >
        <span class="name">{t("details.releaseNotes")}</span>
      </button>
      {#if notesOpen}
        {@render output(notes.notes, notes.notesClipped, true)}
      {/if}
    {/if}
  {/if}
</div>

<style>
  .details {
    padding: 0 12px 10px;
    font-size: 0.85em;
    /* Selectable, unlike the rest of the app, so a path or branch can be copied. */
    -webkit-user-select: text;
    user-select: text;
    cursor: auto;
  }

  .description {
    margin: 0 0 8px;
    color: var(--fg-muted);
    white-space: pre-line;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 3px 12px;
    margin: 0;
  }

  dt {
    color: var(--fg-muted);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* One entry per line, such as a group's members. */
  dd.lines {
    white-space: pre-line;
  }

  .heading {
    margin: 8px 0 3px;
    color: var(--fg-muted);
  }

  .tests {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .test {
    display: flex;
    align-items: baseline;
    gap: 4px;
    width: 100%;
  }

  .test .points {
    margin-inline-start: 8px;
  }

  button.test {
    margin: 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: pointer;
    white-space: normal;
    flex-shrink: 1;
    min-width: 0;
  }

  button.test:hover:not(:disabled),
  .link:hover:not(:disabled) {
    background: none;
  }

  button.test:hover .name {
    text-decoration: underline;
  }

  .chevron {
    flex: none;
    width: 1ch;
    color: var(--fg-muted);
    transition: transform 0.12s ease;
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .output {
    margin: 3px 0 6px;
  }

  .timeout,
  .hint {
    margin: 0 0 4px;
  }

  .hint {
    color: var(--fg-muted);
  }

  pre {
    margin: 0;
    padding: 6px 8px;
    max-height: 16em;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: 0.92em;
    line-height: 1.35;
    white-space: pre;
    tab-size: 4;
  }

  pre.report {
    unicode-bidi: plaintext;
  }

  .link {
    margin: 2px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }

  .link:hover:not(:disabled) {
    text-decoration: underline;
  }

  .tests .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tests .points {
    color: var(--warning);
    font-variant-numeric: tabular-nums;
  }

  .more {
    margin: 3px 0 0;
    color: var(--fg-muted);
  }
</style>
