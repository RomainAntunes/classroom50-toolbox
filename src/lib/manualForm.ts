// A project the student adds by hand (`manual.rs`): the form's draft, what keeps it from
// being saved, and removing one. Loaded with the form or on a menu click, never at startup.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, withDialog, type ManualDraft, type Project } from "#lib/api.js";
import { announce } from "#lib/announce.svelte.js";
import { t } from "#lib/i18n/index.js";
import type { ManualInit } from "#lib/manualEditor.svelte.js";
import { DEFAULT_TIME, localDate } from "#lib/phases.js";

/** The longest name or class Rust accepts (`manual::MAX_NAME`), in UTF-16 units. */
export const MAX_NAME = 80;
/** The longest notes Rust accepts (`manual::MAX_NOTES`), in UTF-16 units. */
export const MAX_NOTES = 4000;

/** The form's fields as the inputs hold them. */
export interface ManualFormDraft {
  name: string;
  class: string;
  /** Whether it has a due date; the date and time fields show only then. */
  hasDue: boolean;
  /** `YYYY-MM-DD`, as `<input type="date">` gives it. */
  date: string;
  /** `HH:MM`, as `<input type="time">` gives it. */
  time: string;
  notes: string;
  done: boolean;
  path: string | null;
  repo: string;
}

/** The field that keeps Save disabled, the first one found. */
export type FormProblem = "name" | "date" | "repo";

const pad = (n: number) => String(n).padStart(2, "0");
const dayOf = (at: Date) => `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;

/** The draft for `init`. Without a due date the date fields still start a week from today:
 *  WebKit's empty date field reports nothing until day, month and year are all typed. */
export function draftFor(init: ManualInit, now: Date): ManualFormDraft {
  const due = init.due ? new Date(init.due) : null;
  const week = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 7);
  return {
    name: init.name,
    class: init.class,
    hasDue: due !== null,
    date: dayOf(due ?? week),
    time: due ? `${pad(due.getHours())}:${pad(due.getMinutes())}` : DEFAULT_TIME,
    notes: init.notes,
    done: init.done,
    path: init.path,
    repo: init.repo,
  };
}

/** `owner/repo` from what was typed: `owner/repo` or a GitHub URL, as Rust's
 *  `manual::parse_repo` reads it; null when it reads as neither. */
export function parseRepo(text: string): string | null {
  const trimmed = text.trim();
  const path = trimmed
    .replace(/^(?:https?|ssh|git):\/\/(?:[^@/]+@)?(?:www\.)?github\.com(?::\d+)?\//i, "")
    .replace(/^git@github\.com:/i, "")
    .replace(/^(?:www\.)?github\.com\//i, "")
    .replace(/\/+$/, "")
    .replace(/\.git$/, "");
  const match = /^([A-Za-z0-9](?:-?[A-Za-z0-9])*)\/([A-Za-z0-9._-]+)$/.exec(path);
  if (!match || match[1].length > 39 || match[2].length > 100) return null;
  if (match[2] === "." || match[2] === "..") return null;
  return `${match[1]}/${match[2]}`;
}

export function formProblem(draft: ManualFormDraft): FormProblem | null {
  if (!draft.name.trim()) return "name";
  if (draft.hasDue && !localDate(draft.date, draft.time)) return "date";
  if (draft.repo.trim() && !parseRepo(draft.repo)) return "repo";
  return null;
}

/** The draft as Rust takes it; call only once `formProblem` finds nothing. */
export function toManualDraft(draft: ManualFormDraft, id: string | null): ManualDraft {
  const due = draft.hasDue ? localDate(draft.date, draft.time) : null;
  return {
    id,
    name: draft.name.trim(),
    class: draft.class.trim() || null,
    due: due ? due.toISOString() : null,
    notes: draft.notes.trim() ? draft.notes : null,
    done: draft.done,
    path: draft.path,
    repo: draft.repo.trim() ? parseRepo(draft.repo) : null,
  };
}

/** Asks, then forgets the manual project; its folder stays as it is. */
export async function removeManual(project: Project): Promise<boolean> {
  const name = project.config.assignment;
  const confirmed = await withDialog(() =>
    ask(t(project.path ? "manual.removeFolder" : "manual.removeText"), {
      title: t("manual.removeTitle", { name }),
      kind: "warning",
      okLabel: t("manual.removeOk"),
      cancelLabel: t("prompt.cancel"),
    }),
  );
  if (!confirmed) return false;
  try {
    await api.removeManualProject(project.id);
    announce(t("manual.removed", { name }));
    return true;
  } catch (err) {
    await withDialog(() => message(errorMessage(err), { title: name, kind: "error" }));
    return false;
  }
}
