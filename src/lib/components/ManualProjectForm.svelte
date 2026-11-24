<script lang="ts">
  // A project the student adds by hand, or one being edited: a name, a class, maybe a due
  // date, notes, a folder and a GitHub repo. Nothing is written before Save; Cancel, Escape
  // or a click outside drop the edits. The date and time fields stay left to right: WebKit
  // reverses their parts in a right-to-left layout.
  import { untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, errorMessage, withDialog } from "#lib/api.js";
  import { announce } from "#lib/announce.svelte.js";
  import { atLeast } from "#lib/busy.js";
  import { t } from "#lib/i18n/index.js";
  import type { ManualInit } from "#lib/manualEditor.svelte.js";
  import {
    MAX_NAME,
    MAX_NOTES,
    draftFor,
    formProblem,
    toManualDraft,
    type ManualFormDraft,
  } from "#lib/manualForm.js";
  import { tildify } from "#lib/paths.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";

  let { init, resolve }: { init: ManualInit; resolve: (saved: boolean) => void } = $props();
  const uid = $props.id();

  const editing = untrack(() => init.id !== null);
  let draft = $state<ManualFormDraft>(untrack(() => draftFor(init, new Date())));
  let saving = $state(false);
  let error = $state<string | null>(null);
  const problem = $derived(formProblem(draft));
  const invalid = (field: string) => problem === field || undefined;
  const describe = (field: string) => (problem === field ? `${uid}-problem` : undefined);

  // The classes already in the list, to pick from as the class is typed.
  const classes = $derived.by(() => {
    const names = new Set<string>();
    for (const project of session.snapshot?.projects ?? []) {
      const name = project.manual
        ? project.config.classroom
        : (project.classroom?.name ?? project.config.classroom);
      if (name) names.add(name);
    }
    return [...names].sort((a, b) => a.localeCompare(b));
  });

  async function chooseFolder() {
    const picked = await withDialog(() =>
      open({
        directory: true,
        title: t("manual.folderPick"),
        defaultPath: draft.path ?? undefined,
      }),
    );
    if (typeof picked === "string") draft.path = picked;
  }

  async function save() {
    if (problem || saving) return;
    saving = true;
    error = null;
    try {
      await atLeast(api.saveManualProject(toManualDraft(draft, init.id)));
      announce(t("manual.saved", { name: draft.name.trim() }));
      resolve(true);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      saving = false;
    }
  }
</script>

<Modal labelledby="{uid}-title" describedby="{uid}-hint" onclose={() => resolve(false)}>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <h2 id="{uid}-title">{t(editing ? "manual.editTitle" : "manual.addTitle")}</h2>
    <p id="{uid}-hint">{t("manual.hint")}</p>

    <label class="field">
      <span>{t("manual.name")}</span>
      <input
        data-autofocus
        bind:value={draft.name}
        maxlength={MAX_NAME}
        autocomplete="off"
        aria-invalid={invalid("name")}
        aria-describedby={describe("name")}
      />
    </label>

    <label class="field">
      <span>{t("manual.class")}</span>
      <input
        bind:value={draft.class}
        maxlength={MAX_NAME}
        autocomplete="off"
        list="{uid}-classes"
        placeholder={t("manual.noClass")}
      />
      <datalist id="{uid}-classes">
        {#each classes as name (name)}<option value={name}></option>{/each}
      </datalist>
    </label>

    <div class="field">
      <label class="check">
        <input type="checkbox" bind:checked={draft.hasDue} />
        {t("manual.hasDue")}
      </label>
      {#if draft.hasDue}
        <div class="line">
          <input
            type="date"
            dir="ltr"
            bind:value={draft.date}
            aria-label={t("manual.dueDate")}
            aria-invalid={invalid("date")}
            aria-describedby={describe("date")}
            required
          />
          <input type="time" dir="ltr" bind:value={draft.time} aria-label={t("manual.dueTime")} />
        </div>
      {/if}
    </div>

    <div class="field">
      <span id="{uid}-folder">{t("manual.folder")}</span>
      <div class="line">
        <span class="path"
          >{#if draft.path}<bdi dir="ltr">{tildify(draft.path)}</bdi>{:else}{t(
              "manual.noFolder",
            )}{/if}</span
        >
        <button type="button" onclick={chooseFolder}>{t("manual.folderChoose")}</button>
        {#if draft.path}
          <button type="button" onclick={() => (draft.path = null)}
            >{t("manual.folderClear")}</button
          >
        {/if}
      </div>
    </div>

    <label class="field">
      <span>{t("manual.repo")}</span>
      <input
        bind:value={draft.repo}
        dir="ltr"
        autocomplete="off"
        spellcheck="false"
        placeholder="owner/repo"
        aria-invalid={invalid("repo")}
        aria-describedby={problem === "repo" ? `${uid}-problem` : `${uid}-repo-hint`}
      />
      <span class="hint" id="{uid}-repo-hint">{t("manual.repoHint")}</span>
    </label>

    <label class="field">
      <span>{t("manual.notes")}</span>
      <textarea bind:value={draft.notes} rows="3" maxlength={MAX_NOTES}></textarea>
    </label>

    {#if editing}
      <label class="check">
        <input type="checkbox" bind:checked={draft.done} />
        {t("manual.done")}
      </label>
    {/if}

    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if problem}
      <p class="problem" id="{uid}-problem">{t(`manual.problem.${problem}`)}</p>
    {/if}
    <div class="buttons">
      <button type="button" onclick={() => resolve(false)}>{t("prompt.cancel")}</button>
      <button
        type="submit"
        class="primary"
        disabled={problem !== null || saving}
        aria-busy={saving}
      >
        {#if saving}<Spinner />{/if}{t(editing ? "manual.save" : "manual.addOk")}
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

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .hint {
    color: var(--fg-muted);
    font-size: 0.9em;
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-muted);
  }

  input:not([type="checkbox"]),
  textarea {
    min-width: 0;
    font: inherit;
    color: inherit;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg-control);
    outline: none;
  }

  input:not([type="checkbox"]) {
    height: 28px;
  }

  textarea {
    padding-block: 4px;
    resize: vertical;
  }

  input:not([type="checkbox"]):focus-visible,
  textarea:focus-visible {
    border-color: var(--focus-ring);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }

  input[aria-invalid="true"] {
    border-color: var(--danger);
  }

  input[type="date"] {
    flex: 1;
  }

  input[type="time"] {
    flex: 0 1 auto;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
