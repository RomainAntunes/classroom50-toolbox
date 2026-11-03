<script lang="ts">
  import { api, errorMessage, type TeammateEntry, type TeammateJob } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import { addTeammates } from "#lib/teammate.js";
  import Spinner from "./Spinner.svelte";

  /**
   * Under a row, the classmates Rust is adding in the background: each username waiting,
   * being added, added or why not. "Add another" queues more at any time; Done clears the
   * results once the queue is empty (hiding the popover does too).
   */
  let { job }: { job: TeammateJob } = $props();

  const added = $derived(job.entries.some((e) => e.state === "added"));

  function line({ username: user, state, error }: TeammateEntry): string {
    switch (state) {
      case "waiting":
        return t("teammate.waiting", { user });
      case "adding":
        return t(job.team ? "teammate.adding" : "teammate.inviting", { user });
      case "added":
        return t(job.team ? "teammate.added" : "teammate.invited", { user });
      case "failed":
        return t("teammate.failed", { user, error: errorMessage(error) });
    }
  }

  function more() {
    void addTeammates({
      id: job.id,
      repo: job.repo,
      assignment: job.assignment,
      team: job.team,
    });
  }
</script>

<section
  class="teammates"
  aria-label={t(job.team ? "teammate.teamTitle" : "teammate.inviteTitle")}
  aria-busy={job.running}
>
  <ul>
    {#each job.entries as entry, i (i)}
      <li class={entry.state}>
        {#if entry.state === "adding"}<Spinner />{/if}{line(entry)}
      </li>
    {/each}
  </ul>
  {#if !job.running && job.team && added}
    <p class="hint">{t("teammate.acceptNext", { assignment: job.assignment })}</p>
  {/if}
  <div class="actions">
    <button class="link" onclick={more}>{t("teammate.again")}</button>
    {#if !job.running}
      <button class="link" onclick={() => void api.dismissTeammateJob(job.id)}
        >{t("teammate.done")}</button
      >
    {/if}
  </div>
</section>

<style>
  .teammates {
    padding: 0 12px 8px;
    font-size: 0.85em;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    overflow-wrap: anywhere;
  }

  .waiting,
  .hint {
    color: var(--fg-muted);
  }

  .added {
    color: var(--success);
  }

  .failed {
    color: var(--danger);
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
    margin: 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }

  .link:hover:not(:disabled) {
    background: none;
    text-decoration: underline;
  }
</style>
