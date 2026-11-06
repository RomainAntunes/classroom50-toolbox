<script lang="ts">
  import { api, errorMessage, type TeammateEntry, type TeammateJob } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import { addMore } from "#lib/teammate.js";
  import Spinner from "./Spinner.svelte";

  type State = TeammateEntry["state"];

  /**
   * Under a row, the classmates Rust is adding in the background: each username waiting,
   * being added, added or why not. It starts folded to one line of counts, which keeps
   * following the queue and names every classmate on hover; the heading unfolds it. "Add another" queues more at any time; Done clears the results
   * once the queue is empty (hiding the popover does too).
   */
  let { job }: { job: TeammateJob } = $props();

  let folded = $state(true);

  const added = $derived(job.entries.some((e) => e.state === "added"));
  const title = $derived(t(job.team ? "teammate.teamTitle" : "teammate.inviteTitle"));

  /** The folded line: how many are added, failed and still to go (shown as being added
   *  while one is). */
  const counts = $derived.by(() => {
    const count = (...states: State[]) =>
      job.entries.filter((e) => states.includes(e.state)).length;
    const pending: State = job.entries.some((e) => e.state === "adding") ? "adding" : "waiting";
    const all: [State, number][] = [
      ["added", count("added")],
      ["failed", count("failed")],
      [pending, count("waiting", "adding")],
    ];
    return all.filter(([, n]) => n > 0);
  });

  function status(state: State): string {
    switch (state) {
      case "waiting":
        return t("teammate.status.waiting");
      case "adding":
        return t(job.team ? "teammate.status.adding" : "teammate.status.inviting");
      case "added":
        return t(job.team ? "teammate.status.added" : "teammate.status.invited");
      case "failed":
        return t("teammate.status.failed");
    }
  }
</script>

{#snippet icon(state: State)}
  <span class="icon">
    {#if state === "adding"}
      <Spinner alone />
    {:else}
      <svg viewBox="0 0 16 16" aria-hidden="true">
        {#if state === "waiting"}
          <circle cx="8" cy="8" r="6" /><path d="M8 4.8V8l2.2 1.4" />
        {:else if state === "added"}
          <path d="m3.5 8.4 2.9 2.9 6.1-6.6" />
        {:else}
          <path d="m4.5 4.5 7 7m0-7-7 7" />
        {/if}
      </svg>
    {/if}
  </span>
{/snippet}

<section class="teammates" aria-label={title} aria-busy={job.running}>
  <button
    class="heading"
    aria-expanded={!folded}
    title={folded ? job.entries.map((e) => e.username).join(", ") : undefined}
    onclick={() => (folded = !folded)}
  >
    <svg class="chevron mirror-rtl" class:open={!folded} viewBox="0 0 24 24" aria-hidden="true"
      ><path d="m9 18 6-6-6-6" /></svg
    >
    <span class="title">{title}</span>
    {#if folded}
      {#each counts as [state, n] (state)}
        <span class="count {state}"
          >{@render icon(state)}{n}<span class="sr-only">{" "}{status(state)}{" "}</span></span
        >
      {/each}
    {/if}
  </button>
  {#if !folded}
    <ul>
      {#each job.entries as entry, i (i)}
        <li class={entry.state}>
          {@render icon(entry.state)}
          <bdi class="user">{entry.username}</bdi>
          <span class="value"
            ><span class="status">{status(entry.state)}</span
            >{#if entry.state === "failed" && entry.error}{" · "}<span class="reason"
                >{errorMessage(entry.error)}</span
              >{/if}</span
          >
        </li>
      {/each}
    </ul>
    {#if !job.running && job.team && added}
      <p class="hint">{t("teammate.acceptNext", { assignment: job.assignment })}</p>
    {/if}
    <div class="actions">
      <button class="link" onclick={() => void addMore(job)}>{t("teammate.again")}</button>
      {#if !job.running}
        <button class="link" onclick={() => void api.dismissTeammateJob(job.id)}
          >{t("teammate.done")}</button
        >
      {/if}
    </div>
  {/if}
</section>

<style>
  /* Laid out like the row's details: the same small text, a muted heading, link actions.
     Folded, the line keeps as much space under it as the row's text has above it. Scoped
     by the root, so the elements inside need no class of their own. */
  .teammates {
    padding: 0 12px 8px;
    font-size: 0.85em;
  }

  .teammates :global {
    button {
      margin: 0;
      padding: 0;
      border: none;
      border-radius: 0;
      background: none;
      font: inherit;
    }

    /* The fold button, drawn as the details' muted heading with a section's chevron. */
    .heading {
      display: flex;
      align-items: center;
      gap: 6px;
      width: 100%;
      color: var(--fg-muted);
      text-align: start;
      cursor: default;
    }

    .heading[aria-expanded="true"] {
      margin-block-end: 3px;
    }

    .chevron {
      flex: none;
      width: 8px;
      height: 8px;
      stroke-width: 3;
      transition: transform 0.15s ease-out;
    }

    .open {
      transform: rotate(90deg);
    }

    .title {
      margin-inline-end: 4px;
    }

    .count {
      display: inline-flex;
      align-items: center;
      gap: 3px;
    }

    svg {
      fill: none;
      stroke: currentColor;
      stroke-linecap: round;
      stroke-linejoin: round;
    }

    /* A name column, each status right beside it (a failure's reason after it, wrapping).
       Where subgrid is missing each line sizes its own name. */
    ul,
    li {
      display: grid;
      grid-template-columns: 1em minmax(0, max-content) minmax(0, 1fr);
      column-gap: 6px;
    }

    ul {
      row-gap: 3px;
      margin: 0;
      padding: 0;
      list-style: none;
    }

    li {
      grid-column: 1 / -1;
      align-items: baseline;
    }

    @supports (grid-template-columns: subgrid) {
      li {
        grid-template-columns: subgrid;
      }
    }

    .icon {
      display: grid;
      place-items: center;
      align-self: center;
      width: 1em;
      height: 1em;
    }

    .icon svg {
      width: 1em;
      height: 1em;
      stroke-width: 1.8;
    }

    .user {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      padding-inline-end: 6px;
    }

    .value {
      overflow-wrap: anywhere;
    }

    /* Each state's color, on its icon and status word. */
    .waiting,
    .adding {
      --tone: var(--fg-muted);
    }

    .added {
      --tone: var(--success);
    }

    .failed {
      --tone: var(--danger);
    }

    .icon,
    .status,
    .count {
      color: var(--tone);
    }

    .reason,
    .hint {
      color: var(--fg-muted);
    }

    .hint {
      margin: 3px 0 0;
    }

    .actions {
      display: flex;
      gap: 12px;
      margin-block-start: 3px;
    }

    .link {
      color: var(--accent-text);
      cursor: pointer;
    }

    .link:hover {
      text-decoration: underline;
    }
  }
</style>
