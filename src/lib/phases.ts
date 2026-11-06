// The phases a student sets on a project (`phases.rs`): what the row, its details and the
// tray read, and the editor's draft, which holds the date and time as the inputs do.

import type { Project, ProjectPhase } from "#lib/api.js";

/** A phase being edited: the inputs' own values, local date and time. */
export interface PhaseDraft {
  id: string;
  name: string;
  /** `YYYY-MM-DD`, as `<input type="date">` gives it; empty until picked. */
  date: string;
  /** `HH:MM`, as `<input type="time">` gives it. */
  time: string;
  done: boolean;
}

/** What keeps the draft from being saved, the first one found, with the phase it is on. */
export interface DraftProblem {
  kind: "name" | "date";
  index: number;
}

/** The longest name Rust accepts (`phases::MAX_NAME`), in UTF-16 units. */
export const MAX_NAME = 80;
/** As many as Rust accepts (`phases::MAX_PHASES`). */
export const MAX_PHASES = 30;
/** A new phase's time: the end of the day, as teachers mostly mean a date. */
export const DEFAULT_TIME = "23:59";

/** The phases that may still remind: not done, on an assignment that isn't closed or
 *  archived (as Rust's `phases::open_phases`). */
export function openPhases(project: Project): ProjectPhase[] {
  const closed = Boolean(project.assignment?.closed || project.remote?.archived);
  return closed ? [] : (project.phases ?? []).filter((p) => !p.done);
}

/** The earliest open phase, past or not: the row's date line without a due date. */
export function nextPhase(project: Project): ProjectPhase | null {
  let next: ProjectPhase | null = null;
  for (const phase of openPhases(project)) {
    if (!next || Date.parse(phase.due) < Date.parse(next.due)) next = phase;
  }
  return next;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** `at`'s local day as `<input type="date">` holds it. */
const dayOf = (at: Date) => `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`;

export function draftFrom(phases: readonly ProjectPhase[]): PhaseDraft[] {
  return phases.map((phase) => {
    const at = new Date(phase.due);
    return {
      id: phase.id,
      name: phase.name,
      date: dayOf(at),
      time: `${pad(at.getHours())}:${pad(at.getMinutes())}`,
      done: phase.done,
    };
  });
}

/** A new phase's id: 32 hex digits. `crypto.randomUUID` needs a secure context, which a
 *  webview served from `http://tauri.localhost` may not count as. */
export function newId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** A new phase, dated `date`. */
export function newDraft(id: string, name: string, date: string): PhaseDraft {
  return { id, name, date, time: DEFAULT_TIME, done: false };
}

/** A new phase's date: a week after the latest phase, or after today when that is later.
 *  A date field that starts empty in WebKit stays empty until its day, month and year are
 *  all typed, so a new phase starts dated and picking a day keeps the month and year. */
export function nextDate(drafts: readonly PhaseDraft[], now: Date): string {
  let base = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  for (const draft of drafts) {
    const at = localDate(draft.date, "00:00");
    if (at && at > base) base = at;
  }
  base.setDate(base.getDate() + 7);
  return dayOf(base);
}

/** The local date and time the inputs hold, or null when they don't make one (a typed
 *  `2026-02-31`, a WebKit without date pickers given free text). */
export function localDate(date: string, time: string): Date | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  const t = /^(\d{2}):(\d{2})$/.exec(time || DEFAULT_TIME);
  if (!d || !t) return null;
  const [year, month, day, hour, minute] = [d[1], d[2], d[3], t[1], t[2]].map(Number);
  const at = new Date(year, month - 1, day, hour, minute);
  const same =
    at.getFullYear() === year &&
    at.getMonth() === month - 1 &&
    at.getDate() === day &&
    at.getHours() === hour &&
    at.getMinutes() === minute;
  return same ? at : null;
}

export function draftProblem(drafts: readonly PhaseDraft[]): DraftProblem | null {
  const name = drafts.findIndex((d) => !d.name.trim());
  if (name >= 0) return { kind: "name", index: name };
  const date = drafts.findIndex((d) => !localDate(d.date, d.time));
  if (date >= 0) return { kind: "date", index: date };
  return null;
}

/** The draft as Rust takes it, by date; call only once `draftProblem` finds nothing. */
export function toPhases(drafts: readonly PhaseDraft[]): ProjectPhase[] {
  return drafts
    .map((d) => ({
      id: d.id,
      name: d.name.trim(),
      due: (localDate(d.date, d.time) ?? new Date(NaN)).toISOString(),
      done: d.done,
    }))
    .sort((a, b) => Date.parse(a.due) - Date.parse(b.due));
}
