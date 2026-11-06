<script lang="ts">
  // The phases of one project: a name, a date and time and a Done box each, saved together.
  // Nothing is written before Save; Cancel, Escape or a click outside drop the edits. The
  // date and time fields stay left to right: WebKit reverses their parts in a right-to-left
  // layout.
  import { tick, untrack } from "svelte";
  import { api, errorMessage, type Project } from "#lib/api.js";
  import { announce } from "#lib/announce.svelte.js";
  import { atLeast } from "#lib/busy.js";
  import { t } from "#lib/i18n/index.js";
  import {
    MAX_NAME,
    MAX_PHASES,
    draftFrom,
    draftProblem,
    newDraft,
    newId,
    nextDate,
    toPhases,
    type PhaseDraft,
  } from "#lib/phases.js";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";

  let { project, resolve }: { project: Project; resolve: (saved: boolean) => void } = $props();

  const name = $derived(project.assignment?.name ?? project.config.assignment);
  let drafts = $state<PhaseDraft[]>(untrack(() => draftFrom(project.phases)));
  let saving = $state(false);
  let error = $state<string | null>(null);
  let list = $state<HTMLElement | null>(null);
  let addButton = $state<HTMLButtonElement | null>(null);
  const problem = $derived(draftProblem(drafts));
  const phaseLabel = (index: number) =>
    drafts[index]?.name.trim() || t("phases.defaultName", { n: index + 1 });
  /** Whether the field of `kind` on phase `index` is what keeps Save disabled. */
  const invalid = (kind: "name" | "date", index: number) =>
    problem?.kind === kind && problem.index === index;

  /** Focuses the name field of phase `index` once rendered, else Add. A new phase's name
   *  is selected, so typing replaces "Phase 2". */
  async function focusName(index: number, select = false) {
    await tick();
    const field = list?.querySelectorAll<HTMLInputElement>("input[data-name]")[index];
    if (!field) return addButton?.focus();
    field.focus();
    if (select) field.select();
  }

  function add() {
    if (drafts.length >= MAX_PHASES) return;
    const name = t("phases.defaultName", { n: drafts.length + 1 });
    drafts.push(newDraft(newId(), name, nextDate(drafts, new Date())));
    void focusName(drafts.length - 1, true);
  }

  function remove(index: number) {
    drafts.splice(index, 1);
    void focusName(Math.min(index, drafts.length - 1));
  }

  async function save() {
    if (problem || saving) return;
    saving = true;
    error = null;
    try {
      await atLeast(api.setPhases(project.id, toPhases(drafts)));
      announce(t("phases.saved", { name }));
      resolve(true);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      saving = false;
    }
  }
</script>

<Modal labelledby="phases-title" describedby="phases-hint" onclose={() => resolve(false)}>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <h2 id="phases-title"><bdi>{t("phases.title", { name })}</bdi></h2>
    <p id="phases-hint">{t("phases.hint")}</p>
    {#if drafts.length === 0}
      <p>{t("phases.empty")}</p>
    {:else}
      <ul bind:this={list}>
        {#each drafts as draft, index (draft.id)}
          {@const label = draft.name.trim() || t("phases.defaultName", { n: index + 1 })}
          {@const badName = invalid("name", index)}
          {@const badDate = invalid("date", index)}
          <li>
            <div class="line">
              <input
                class="name"
                data-name
                data-autofocus={index === 0 ? "" : undefined}
                bind:value={draft.name}
                aria-label={t("phases.nameLabel", { n: index + 1 })}
                aria-invalid={badName || undefined}
                aria-describedby={badName ? "phases-problem" : undefined}
                maxlength={MAX_NAME}
                autocomplete="off"
              />
              <button
                type="button"
                class="icon"
                aria-label={t("phases.delete", { name: label })}
                title={t("phases.delete", { name: label })}
                onclick={() => remove(index)}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6" />
                </svg>
              </button>
            </div>
            <div class="line">
              <input
                type="date"
                dir="ltr"
                bind:value={draft.date}
                aria-label={t("phases.dateLabel", { name: label })}
                aria-invalid={badDate || undefined}
                aria-describedby={badDate ? "phases-problem" : undefined}
                required
              />
              <input
                type="time"
                dir="ltr"
                bind:value={draft.time}
                aria-label={t("phases.timeLabel", { name: label })}
              />
              <label class="done">
                <input type="checkbox" bind:checked={draft.done} />
                {t("phases.done")}
              </label>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    <button
      type="button"
      class="add"
      bind:this={addButton}
      data-autofocus={drafts.length === 0 ? "" : undefined}
      disabled={drafts.length >= MAX_PHASES}
      onclick={add}>{t("phases.add")}</button
    >
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if problem}
      <p class="problem" id="phases-problem">
        {t(`phases.problem.${problem.kind}`, { name: phaseLabel(problem.index) })}
      </p>
    {/if}
    <div class="buttons">
      <button type="button" onclick={() => resolve(false)}>{t("prompt.cancel")}</button>
      <button
        type="submit"
        class="primary"
        disabled={problem !== null || saving}
        aria-busy={saving}
      >
        {#if saving}<Spinner />{/if}{t("phases.save")}
      </button>
    </div>
  </form>
</Modal>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }

  h2 {
    margin: 0;
    font-size: 1em;
    font-weight: 600;
  }

  p {
    margin: 0;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .error {
    color: var(--danger);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-block-end: var(--space);
    border-block-end: 1px solid var(--border);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  input:not([type="checkbox"]) {
    min-width: 0;
    height: 28px;
    font: inherit;
    color: inherit;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  input:not([type="checkbox"]):focus-visible {
    border-color: var(--focus-ring);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }

  input[aria-invalid="true"] {
    border-color: var(--danger);
  }

  .name {
    flex: 1;
  }

  input[type="date"] {
    flex: 1;
  }

  input[type="time"] {
    flex: 0 1 auto;
  }

  .done {
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .add {
    align-self: flex-start;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
