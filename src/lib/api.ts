// Typed wrappers for the Rust commands in src-tauri/src/commands.rs. Keep the two in sync:
// Rust serializes with camelCase field names.
import { invoke } from "@tauri-apps/api/core";

export type Os = "macos" | "windows" | "linux";
export type Tool = "git" | "gh" | "gh-student";

export interface AppInfo {
  os: Os;
  version: string;
}

export interface ToolStatus {
  tool: Tool;
  installed: boolean;
  version: string | null;
}

export interface AuthStatus {
  loggedIn: boolean;
  login: string | null;
}

export interface CommandSpec {
  program: string;
  args: string[];
}

export interface RepoConfig {
  classroom: string;
  assignment: string;
  secret: string | null;
}

export interface Commit {
  oid: string;
  at: string | null;
}

export interface Submission {
  tag: string;
  commit: string;
  at: string | null;
}

export interface RemoteRepo {
  owner: string;
  name: string;
  url: string;
  archived: boolean;
  canPush: boolean;
  defaultBranch: string | null;
  head: Commit | null;
  lastSubmission: Submission | null;
  config: RepoConfig | null;
  /** Branches with commits missing from the default branch. */
  unmerged: Branch[];
  grade: Grade | null;
  feedback: Feedback | null;
}

export interface Branch {
  name: string;
  commits: number;
}

/** The newest autograder result (`result.json` on the `submit/*` release). */
export interface Grade {
  tag: string;
  score: number;
  maxScore: number;
  testsPassed: number;
  testsTotal: number;
  publishedAt: string | null;
  url: string;
}

/** Teacher comments and reviews on the Feedback PR. */
export interface Feedback {
  url: string;
  comments: number;
  latestAt: string | null;
}

export interface Classroom {
  org: string;
  slug: string;
  name: string | null;
  term: string | null;
  active: boolean;
  secret: string | null;
  pagesBaseUrl: string | null;
}

export interface LocalState {
  branch: string | null;
  head: string | null;
  ahead: number | null;
  behind: number | null;
  changedFiles: number;
  /** Local branches with commits missing from the local default branch. */
  unmerged: string[];
  /** GitHub has commits the clone doesn't (someone pushed since the last pull). */
  remoteAhead: boolean;
}

export type Pending = "uncommitted" | "unpushed" | "unmerged" | "unsubmitted";

export interface Assignment {
  slug: string;
  name: string;
  mode: "individual" | "group" | "team";
  description: string | null;
  due: string | null;
  availableFrom: string | null;
  submissionMode: "every-push" | "tag" | null;
  locked: boolean;
  closed: boolean;
  feedbackPr: boolean;
  passThreshold: number | null;
}

/** One assignment repo, merged from GitHub, the watched folders and the Pages manifest. */
export interface Project {
  id: string;
  owner: string | null;
  repo: string | null;
  config: RepoConfig;
  path: string | null;
  local: LocalState | null;
  remote: RemoteRepo | null;
  classroom: Classroom | null;
  assignment: Assignment | null;
  pending: Pending | null;
  /** The assignment's page in the Classroom 50 web app. */
  webUrl: string | null;
}

/** A published assignment the user hasn't accepted yet. */
export interface Unaccepted {
  id: string;
  classroom: Classroom;
  assignment: Assignment;
  acceptUrl: string;
}

export interface Snapshot {
  projects: Project[];
  unaccepted: Unaccepted[];
  /** Last full sync. */
  syncedAt: string | null;
  /** Last GitHub read, full sync or status check. */
  checkedAt: string | null;
  /** Why the last sync was partial; same shape as `CommandError`. */
  issue: CommandError | null;
}

export interface Settings {
  watchedFolders: string[];
  scanEditorProjects: boolean;
  launchAtLogin: boolean;
  notifyHoursBefore: number[];
}

/** Payload of the `login-device-code` event emitted during `login`. */
export interface DeviceCode {
  code: string;
  url: string;
}

export const DEVICE_CODE_EVENT = "login-device-code";
export const UPDATE_AVAILABLE_EVENT = "update-available";
export const PROJECTS_UPDATED_EVENT = "projects-updated";
export const SYNC_PROGRESS_EVENT = "sync-progress";
/** Payload: `Notice`. The UI words it and sends the system notification. */
export const NOTIFY_EVENT = "notify";
/** Payload: ids of projects and unaccepted assignments with unseen news. */
export const UNSEEN_EVENT = "unseen-updated";

export type Notice =
  | { kind: "due"; assignment: string; due: string }
  | { kind: "grade"; assignment: string; score: number; maxScore: number }
  | { kind: "feedback"; assignment: string; count: number }
  | { kind: "newAssignment"; assignment: string; classroom: string };

/** Payload of `sync-progress`. `total` grows as work is found; 0 means not known yet. */
export interface SyncProgress {
  running: boolean;
  done: number;
  total: number;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** Shape of every error a command rejects with (see `Error` in src-tauri/src/error.rs). */
export interface CommandError {
  kind: string;
  message: string;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  setPopoverPinned: (pinned: boolean) => invoke<void>("set_popover_pinned", { pinned }),
  toolStatuses: () => invoke<ToolStatus[]>("tool_statuses"),
  installCommand: (tool: Tool) => invoke<CommandSpec>("install_command", { tool }),
  installTool: (tool: Tool) => invoke<ToolStatus>("install_tool", { tool }),
  authStatus: () => invoke<AuthStatus>("auth_status"),
  listProjects: () => invoke<Snapshot>("list_projects"),
  refreshProjects: () => invoke<Snapshot>("refresh_projects"),
  syncProgress: () => invoke<SyncProgress>("sync_progress"),
  rescanLocal: () => invoke<Snapshot>("rescan_local"),
  unseen: () => invoke<string[]>("unseen"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  login: () => invoke<AuthStatus>("login"),
  cancelLogin: () => invoke<void>("cancel_login"),
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
};

export function errorKind(err: unknown): string | null {
  return typeof err === "object" && err !== null && "kind" in err
    ? String((err as CommandError).kind)
    : null;
}

export function errorMessage(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    return String((err as CommandError).message);
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
