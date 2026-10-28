// The Rust commands in src-tauri/src/commands/, typed from `bindings.ts`, which the
// `bindings` test generates from the Rust types (`UPDATE_GOLDEN=1 cargo test bindings`
// rewrites it). A command, argument, field or variant renamed or retyped in Rust fails
// svelte-check wherever the UI relies on it. Types keep the names the UI always used.
// Use only the aliases below (built on the `_Serialize` halves, what Rust sends): never import
// an un-suffixed split type such as `B.Snapshot` or a `_Deserialize` half from bindings.ts,
// since specta renders the field aliases Rust accepts when reading with the wrong precedence.
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { commands } from "#lib/bindings.js";
import type * as B from "#lib/bindings.js";
import { hasKey, t } from "#lib/i18n/index.js";

export type Os = B.Os;
export type Tool = B.Tool;
export type AppInfo = B.AppInfo;
/** What the UI tells `diagnostics_report` about itself. */
export type DiagnosticsUi = B.UiFacts;
/** The "Copy diagnostics" report, names already masked. */
export type DiagnosticsReport = B.Report;
/** A calendar of the OS's calendar app the user can write to. */
export type NativeCalendar = B.NativeCalendar_Serialize;
export type ToolStatus = B.ToolStatus;
/** An install or update still running (`tool_jobs`, the `tool-jobs` event). */
export type ToolJob = B.ToolJob;
export type AuthStatus = B.AuthStatus;
/** A github.com account gh is signed in to. */
export type Account = B.Account;
/** How a tool installs here (`install_command`). */
export type InstallPlan = B.InstallPlan;
export type CommandSpec = Omit<InstallPlan, "manual">;
export type RepoConfig = B.RepoConfig;
export type Commit = B.Commit;
export type Submission = B.Submission;
export type RemoteRepo = B.RemoteRepo;
export type Grading = B.Grading;
export type Branch = B.Branch;
/** The newest autograder result (`result.json` on the `submit/*` release). */
export type Grade = B.Grade;
/** A past grade: the score without the test list. */
export type GradeSummary = B.GradeSummary;
export type GradeTest = B.GradeTest;
/** The release body's text that isn't a test's output (a custom autograder's report). */
export type ReleaseNotes = B.ReleaseNotes;
/** Teacher comments and reviews on the Feedback PR. */
export type Feedback = B.Feedback;
export type Classroom = B.Classroom;
export type LocalState = B.LocalState;
export type Pending = B.Pending;
export type Assignment = B.Assignment_Serialize;
/** One assignment repo, merged from GitHub, the watched folders and the Pages manifest. */
export type Project = B.Project_Serialize;
/** A group team (`classroom50-group-<hash>-<n>`) the user is on. */
export type Group = B.Group;
/** What adding one classmate gave (`add_teammates`). */
export type Added = B.Added_Serialize;
export type Member = B.Member;
/** The words of calendar event notes; `{name}`, `{n}`, `{score}`, `{max}` stay as is. */
export type CalendarLabels = B.CalendarLabels;
/** The tray icon's menu, computed and worded by the UI (`src/lib/tray.ts`). */
export type TrayMenu = B.TrayMenu;
export type TrayItem = B.TrayItem;
/** A tray menu item the UI carries out (`tray-action`). */
export type TrayAction = B.Action;
/** A published assignment the user hasn't accepted yet. */
export type Unaccepted = B.Unaccepted_Serialize;
/** A `classroom50-toolbox://` link, validated by Rust. */
export type DeepLink = B.Link;
/** Where a link leads (`take_deep_link`). */
export type DeepLinkTarget = B.Target_Serialize;
/** The OS's 12- or 24-hour clock, as `Intl.DateTimeFormat`'s `hourCycle`. */
export type HourCycle = B.HourCycle;
/** How gh clones github.com repos. */
export type GitProtocol = B.GitProtocol;
export type GitCredentials = B.GitCredentials;
/** A Linux calendar store (`NativeCalendar.kind`), worded as `settings.calendar.store.<kind>`. */
export type CalendarStore = B.CalendarStore;
/** Result of `accept_assignment`. */
export type Accepted = B.Accepted;
/** What a submit would send (`submit_check`). */
export type SubmitCheck = B.SubmitCheck;
export type Submitted = B.Submitted;
/** What Commit & push would do (`push_check`). */
export type PushCheck = B.PushCheck;
/** What Commit would do (`commit_check`), and whether Commit & push is offered with it. */
export type CommitCheck = B.CommitCheck;
export type Pushed = B.Pushed;
/** Who to commit as: the signed-in account and its private GitHub address. */
export type GitIdentity = B.Identity;
export type Snapshot = B.Snapshot_Serialize;
export type Appearance = B.Appearance;
/** The git writes the app offers on a clone, from none to commit and push. */
export type CommitActions = B.CommitActions;
/** Which releases self-update follows: published stable ones, or pre-releases too. */
export type UpdateChannel = B.UpdateChannel;
export type Settings = B.Settings;
/** An editor installed here that can open a folder. */
export type EditorInfo = B.EditorInfo;
/** The date filter above the project list. */
export type ListFilter = B.ListFilter;
/** Payload of the `login-device-code` event emitted during `login`. */
export type DeviceCode = B.DeviceCode;
export type Notice = B.Notice;
export type SimulatedNetwork = B.Network;
/** Payload of `sync-progress`. */
export type SyncProgress = B.SyncProgress;
export type LastRun = B.LastRun;
export type RateLimit = B.RateLimit;
/** What the sync engine did lately (Settings). */
export type SyncDetails = B.SyncDetails_Serialize;
/** Where self-update stands (`updater.rs`). */
export type UpdateState = B.UpdateState;
/** How the app was installed, which decides what updating can do. */
export type Bundle = B.Bundle;
/** Payload of `update-state` and `update_status`. */
export type UpdateStatus = B.UpdateStatus;
export type ErrorKind = B.ErrorKind;
/** Shape of every error a command rejects with (`CommandError` in src-tauri/src/error.rs). */
export type CommandError = B.CommandError_Serialize;

/** `values` lists every member of `T`: a variant added in Rust fails svelte-check here. */
function every<T extends string>() {
  return <const A extends readonly T[]>(values: A & ([T] extends [A[number]] ? unknown : never)) =>
    values;
}

export const APPEARANCES = every<Appearance>()(["system", "light", "dark"]);
export const COMMIT_ACTIONS = every<CommitActions>()(["none", "commit", "push"]);
export const UPDATE_CHANNELS = every<UpdateChannel>()(["stable", "beta"]);
export const CALENDAR_STORES = every<CalendarStore>()(["gnome", "kde"]);
export const SIMULATED_NETWORKS = every<SimulatedNetwork>()(["normal", "githubDown", "noInternet"]);

/** Every event Rust sends, by name, with its payload (`src-tauri/src/events.rs`). */
export type Events = B.Events_Serialize;
export type EventName = keyof Events;

export const DEVICE_CODE_EVENT = "login-device-code" satisfies EventName;
/** However a sign-in ends (success, failure or cancel). */
export const LOGIN_ENDED_EVENT = "login-ended" satisfies EventName;
/** Payload: `UpdateStatus`, on every change of the self-update state. */
export const UPDATE_STATE_EVENT = "update-state" satisfies EventName;
/** Payload: `ToolJob[]`, every install or update running. */
export const TOOL_JOBS_EVENT = "tool-jobs" satisfies EventName;
export const PROJECTS_UPDATED_EVENT = "projects-updated" satisfies EventName;
export const SYNC_PROGRESS_EVENT = "sync-progress" satisfies EventName;
/** Payload: `Notice`. The UI words it and sends the system notification. */
export const NOTIFY_EVENT = "notify" satisfies EventName;
/** Payload: ids of projects and unaccepted assignments with unseen news. */
export const UNSEEN_EVENT = "unseen-updated" satisfies EventName;
/** Payload: `Settings` Rust changed by itself (hidden ids moved by a rename). */
export const SETTINGS_UPDATED_EVENT = "settings-updated" satisfies EventName;
/** A `classroom50-toolbox://` link is waiting; `takeDeepLink` reads it. */
export const DEEP_LINK_EVENT = "deep-link" satisfies EventName;
/** The popover hid. */
export const POPOVER_HIDDEN_EVENT = "popover-hidden" satisfies EventName;
/** The popover showed. */
export const POPOVER_SHOWN_EVENT = "popover-shown" satisfies EventName;
/** The global shortcut showed the popover: the list takes the keyboard focus. */
export const POPOVER_SHORTCUT_EVENT = "popover-shortcut" satisfies EventName;
/** Payload: `TrayAction`, a tray menu item for the UI to carry out. */
export const TRAY_ACTION_EVENT = "tray-action" satisfies EventName;
/** The tray menu's `validUntil` passed: it is worded again. */
export const TRAY_EXPIRED_EVENT = "tray-expired" satisfies EventName;
/** Payload: the OS's `HourCycle` (or `null`), when it changed since `systemHourCycle`. */
export const HOUR_CYCLE_EVENT = "hour-cycle" satisfies EventName;

/** Listens to a Rust event, its payload typed by `Events`. */
export function onEvent<K extends EventName>(
  name: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  return listen<Events[K]>(name, (event) => handler(event.payload));
}

/** Every command; each rejects with a `CommandError`. */
export const api = commands;

export function errorKind(err: unknown): ErrorKind | null {
  return typeof err === "object" && err !== null && "kind" in err
    ? ((err as CommandError).kind as ErrorKind)
    : null;
}

/** The error in the UI's language when `en.json` words its kind, else Rust's message. */
export function errorMessage(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    const { kind, message, params } = err as CommandError;
    const key = `error.${kind}`;
    if (!hasKey(key)) return String(message);
    // A param missing (a sync issue saved by an older version): Rust's words are complete.
    const text = t(key, params ?? {});
    return /\{\w+\}/.test(text) ? String(message) : text;
  }
  return String(err);
}

/**
 * Runs `show` (a native dialog) with the popover pinned, so it doesn't hide when the dialog
 * takes focus. Wrap every `open`/`ask`/`message` call from the dialog plugin in this.
 */
export async function withDialog<T>(show: () => Promise<T>): Promise<T> {
  await api.setPopoverPinned(true);
  try {
    return await show();
  } finally {
    await api.setPopoverPinned(false);
  }
}
