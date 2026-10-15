// Typed wrappers for the Rust commands in src-tauri/src/commands.rs. Keep the two in sync:
// Rust serializes with camelCase field names.
import { invoke } from "@tauri-apps/api/core";

export type Os = "macos" | "windows" | "linux";
export type Tool = "git" | "gh" | "gh-student";

export interface AppInfo {
  os: Os;
  version: string;
  /** The OS has a calendar store the app can write into (macOS). */
  nativeCalendar: boolean;
}

/** A calendar of the OS's calendar app the user can write to. */
export interface NativeCalendar {
  id: string;
  title: string;
  /** The account holding it (iCloud, Google, On My Mac...). */
  account: string;
}

export interface ToolStatus {
  tool: Tool;
  installed: boolean;
  version: string | null;
}

/** An install or update still running (`tool_jobs`, the `tool-jobs` event). */
export interface ToolJob {
  tool: Tool;
  kind: "install" | "upgrade";
  /** The command line, shown until it prints something. */
  command: string;
  /** The last line the command printed. */
  line: string | null;
}

export interface AuthStatus {
  loggedIn: boolean;
  login: string | null;
}

/** A github.com account gh is signed in to. */
export interface Account {
  login: string;
  active: boolean;
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
  /** GitHub login of the author, else the name in the commit. */
  author: string | null;
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
  /** Every graded submission still listed (the last 10 releases), newest first. */
  history: GradeSummary[];
  /** The autograder's run on the head commit, while running or when it broke. */
  grading: Grading | null;
  feedback: Feedback | null;
}

export type Grading =
  | { state: "running"; url: string | null }
  | { state: "failed"; url: string | null; message: string | null };

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
  /** Each test, in the autograder's order; why one failed is only on the release page. */
  tests: GradeTest[];
  publishedAt: string | null;
  url: string;
}

/** A past grade: the score without the test list. */
export type GradeSummary = Omit<Grade, "tests">;

export interface GradeTest {
  name: string;
  passed: boolean;
  score: number;
  maxScore: number;
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

export type Pending = "conflict" | "uncommitted" | "unpushed" | "unmerged" | "unsubmitted";

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
  /** `team` mode only: who forms the groups. */
  teamFormation: "teacher" | "student" | null;
  /** `"public"` when accepting creates a public repo. */
  repoVisibility: string | null;
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
  /** How far past the due date the graded work is; advisory, nothing is blocked. */
  lateSeconds: number | null;
  /** The editor that last opened the clone (`EditorInfo.id`), when one did. */
  editor: string | null;
  /** The user's group, on a team assignment. */
  group: Group | null;
}

/** A group team (`classroom50-group-<hash>-<n>`) the user is on. */
export interface Group {
  org: string;
  slug: string;
  /** The `<n>` of the team name, also in the shared repo's name. */
  number: number;
  /** The display name the founders chose, when they did. */
  name: string | null;
  members: Member[];
  /** The team's page on GitHub. */
  url: string;
}

export interface Member {
  login: string;
  name: string | null;
  /** Public GitHub email, else one they commit with in the clone. */
  email: string | null;
}

/** The words of calendar event notes; `{name}`, `{n}`, `{score}`, `{max}` stay as is. */
export interface CalendarLabels {
  class: string;
  group: string;
  groupNumber: string;
  graded: string;
  submitted: string;
  notAccepted: string;
}

/** A published assignment the user hasn't accepted yet. */
export interface Unaccepted {
  id: string;
  classroom: Classroom;
  assignment: Assignment;
  acceptUrl: string;
  /** The group the user is already on (a teacher-formed group, or one they joined). */
  group: Group | null;
}

/** Result of `accept_assignment`. */
export interface Accepted {
  /** `owner/name` as GitHub spells it. */
  repo: string;
  /** Null when the sync after the accept hasn't found the repo yet. */
  projectId: string | null;
}

/** What a submit would send (`submit_check`). */
export interface SubmitCheck {
  /** Submit needs `.classroom50.yaml`; empty-repo assignments are pushed by hand. */
  hasMarker: boolean;
  /** New files that will be included. */
  untracked: string[];
  /** Tracked files deleted without `git rm`: submit would fail. */
  deleted: string[];
}

export interface Submitted {
  /** The submission commit on GitHub. */
  url: string | null;
}

export interface Snapshot {
  projects: Project[];
  unaccepted: Unaccepted[];
  /** Last full sync. */
  syncedAt: string | null;
  /** Last GitHub read, full sync or status check. */
  checkedAt: string | null;
  /** The GitHub account the data belongs to. */
  login: string | null;
  /** Why the last sync was partial; same shape as `CommandError`. */
  issue: CommandError | null;
}

export const APPEARANCES = ["system", "light", "dark"] as const;
export type Appearance = (typeof APPEARANCES)[number];

export interface Settings {
  watchedFolders: string[];
  scanEditorProjects: boolean;
  /** Where the app clones assignments; asked the first time. */
  cloneFolder: string | null;
  launchAtLogin: boolean;
  listFilter: ListFilter;
  notifyHoursBefore: number[];
  /** Editor for clones no editor has opened yet; `null` picks the first one installed. */
  defaultEditor: string | null;
  /** Hidden project and unaccepted-assignment ids: listed only when a search matches. */
  hidden: string[];
  /** Classes (`owner/classroom`, lowercase, see `classKey`) hidden whole. */
  hiddenClasses: string[];
  /** Classes whose section is folded in the list. */
  collapsedClasses: string[];
  appearance: Appearance;
  /** The last exported `.ics`, rewritten by Rust after each sync while set. */
  calendarFile: string | null;
  /** The OS calendar the due dates are kept in. */
  nativeCalendar: string | null;
  /** The first-run guide was finished or skipped. */
  onboarded: boolean;
}

/** An editor installed here that can open a folder. */
export interface EditorInfo {
  id: string;
  name: string;
}

/** The date filter above the project list. */
export type ListFilter = "all" | "open" | "week" | "past";

/** Payload of the `login-device-code` event emitted during `login`. */
export interface DeviceCode {
  code: string;
  url: string;
}

export const DEVICE_CODE_EVENT = "login-device-code";
/** However a sign-in ends (success, failure or cancel). */
export const LOGIN_ENDED_EVENT = "login-ended";
export const UPDATE_AVAILABLE_EVENT = "update-available";
/** Payload: `ToolJob[]`, every install or update running. */
export const TOOL_JOBS_EVENT = "tool-jobs";
export const PROJECTS_UPDATED_EVENT = "projects-updated";
export const SYNC_PROGRESS_EVENT = "sync-progress";
/** Payload: `Notice`. The UI words it and sends the system notification. */
export const NOTIFY_EVENT = "notify";
/** Payload: ids of projects and unaccepted assignments with unseen news. */
export const UNSEEN_EVENT = "unseen-updated";

export type Notice =
  | { kind: "due"; assignment: string; due: string }
  | { kind: "grade"; assignment: string; score: number; maxScore: number }
  | { kind: "gradingFailed"; assignment: string }
  | { kind: "feedback"; assignment: string; count: number }
  | { kind: "newAssignment"; assignment: string; classroom: string }
  | { kind: "teammatePush"; assignment: string; author: string }
  | { kind: "groupJoined"; assignment: string; members: string[] }
  | { kind: "deadlineChanged"; assignment: string; due: string | null }
  | { kind: "closed"; assignment: string };

export const SIMULATED_NETWORKS = ["normal", "githubDown", "noInternet"] as const;
export type SimulatedNetwork = (typeof SIMULATED_NETWORKS)[number];

/** Payload of `sync-progress`. */
export interface SyncProgress {
  running: boolean;
  /** Started by the user; background checks show no spinner. */
  foreground: boolean;
  /** From 0 to 1, each phase weighted by how long it took last time; never steps back. */
  fraction: number;
}

export interface LastRun {
  at: string;
  seconds: number;
}

export interface RateLimit {
  remaining: number;
  limit: number;
  resetAt: string | null;
}

/** What the sync engine did lately (Settings). */
export interface SyncDetails {
  login: string | null;
  syncedAt: string | null;
  checkedAt: string | null;
  stats: {
    rateLimit: RateLimit | null;
    full: LastRun | null;
    status: LastRun | null;
    local: LastRun | null;
    reposSeen: number;
    scanRoots: number;
    watchTargets: number;
  };
  rateLimit: RateLimit | null;
  issue: CommandError | null;
  assignments: number;
  clones: number;
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
  /** Dev builds only: lights the tray badge 3 s from now. */
  previewBadge: () => invoke<void>("preview_badge"),
  /** Writes the due dates as an `.ics` file; returns the number of events. */
  exportCalendar: (path: string) => invoke<number>("export_calendar", { path }),
  setCalendarLabels: (labels: CalendarLabels) => invoke<void>("set_calendar_labels", { labels }),
  /** Asks for calendar access the first time. */
  nativeCalendars: () => invoke<NativeCalendar[]>("native_calendars"),
  /** Writes the due dates into that calendar from now on; `null` removes them. */
  setNativeCalendar: (id: string | null) => invoke<Settings>("set_native_calendar", { id }),
  /** Dev builds only: pretends GitHub is down or the device is offline. */
  simulateNetwork: (network: SimulatedNetwork) => invoke<void>("simulate_network", { network }),
  simulatedNetwork: () => invoke<SimulatedNetwork>("simulated_network"),
  setPopoverPinned: (pinned: boolean) => invoke<void>("set_popover_pinned", { pinned }),
  toolStatuses: () => invoke<ToolStatus[]>("tool_statuses"),
  installCommand: (tool: Tool) => invoke<CommandSpec>("install_command", { tool }),
  /** The newer version `upgradeTool` would install; always null for git and gh. */
  toolUpdate: (tool: Tool) => invoke<string | null>("tool_update", { tool }),
  /** gh-student only; git and gh follow the OS package manager. */
  upgradeTool: (tool: Tool) => invoke<ToolStatus>("upgrade_tool", { tool }),
  /** Every tool's status afterwards (gh-student depends on gh); `tool-jobs` reports output. */
  installTool: (tool: Tool) => invoke<ToolStatus[]>("install_tool", { tool }),
  toolJobs: () => invoke<ToolJob[]>("tool_jobs"),
  /** Stops an install or update; its command then rejects with `cancelled`. */
  cancelToolJob: (tool: Tool) => invoke<void>("cancel_tool_job", { tool }),
  authStatus: () => invoke<AuthStatus>("auth_status"),
  listProjects: () => invoke<Snapshot>("list_projects"),
  refreshProjects: () => invoke<Snapshot>("refresh_projects"),
  syncProgress: () => invoke<SyncProgress>("sync_progress"),
  rescanLocal: () => invoke<Snapshot>("rescan_local"),
  unseen: () => invoke<string[]>("unseen"),
  syncDetails: () => invoke<SyncDetails>("sync_details"),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  switchAccount: (login: string) => invoke<AuthStatus>("switch_account", { login }),
  /** A `data:` URL, cached on disk for a week. */
  accountAvatar: (login: string) => invoke<string>("account_avatar", { login }),
  clearCache: () => invoke<Snapshot>("clear_cache"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  login: () => invoke<AuthStatus>("login"),
  cancelLogin: () => invoke<void>("cancel_login"),
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  installedEditors: () => invoke<EditorInfo[]>("installed_editors"),
  addTeammate: (id: string, username: string) => invoke<void>("add_teammate", { id, username }),
  openInEditor: (path: string, editor: string) => invoke<void>("open_in_editor", { path, editor }),
  /** Runs `gh student accept`, then syncs; rejects with `needsTeam` when a group is needed. */
  acceptAssignment: (id: string, newTeam: boolean) =>
    invoke<Accepted>("accept_assignment", { id, newTeam }),
  submitCheck: (id: string) => invoke<SubmitCheck>("submit_check", { id }),
  /** Runs `gh student submit` in the clone, then re-reads the status while it's graded. */
  submitProject: (id: string) => invoke<Submitted>("submit_project", { id }),
  /** Clones into `parent/<repo>` and returns the new folder; `projects-updated` follows. */
  cloneProject: (id: string, parent: string) => invoke<string>("clone_project", { id, parent }),
  /** Fast-forwards the clone's default branch; rejects with `localChanges`, `otherBranch` or `diverged`. */
  pullProject: (id: string) => invoke<void>("pull_project", { id }),
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
