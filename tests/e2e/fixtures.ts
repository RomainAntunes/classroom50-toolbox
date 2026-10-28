// The fake Rust side's state for one test, typed against the UI's own IPC types. `scenario()`
// gives a signed-in, set-up student with an empty list; the builders below make the rows.
// Everything here is plain data: it is serialized into the page before the app boots.
import type {
  Account,
  AppInfo,
  Assignment,
  AuthStatus,
  Classroom,
  CommandError,
  DeepLinkTarget,
  EditorInfo,
  ErrorKind,
  Grade,
  GradeTest,
  LocalState,
  Project,
  RemoteRepo,
  Settings,
  Snapshot,
  SyncDetails,
  ToolStatus,
  Unaccepted,
  UpdateStatus,
} from "../../src/lib/api";
import pkg from "../../package.json" with { type: "json" };

/** How the fake answers one call: a value, a rejection shaped like Rust's `Error` (a plugin
 *  rejects with its message alone), a promise the test settles later (`app.release`), or the
 *  fake's default (`pass`). */
export type Reply =
  { value: unknown } | { error: CommandError | string } | { hold: true } | { pass: true };

export interface Scenario {
  appInfo: AppInfo;
  settings: Settings;
  tools: ToolStatus[];
  auth: AuthStatus;
  snapshot: Snapshot;
  editors: EditorInfo[];
  accounts: Account[];
  systemLanguages: string[];
  unseen: string[];
  syncDetails: SyncDetails;
  /** Self-update (`update_status`); `defer_update` and `install_update` change it and send
   *  `update-state`, as Rust does. */
  update: UpdateStatus;
  /** What `take_deep_link` hands over once (then null). */
  deepLink: DeepLinkTarget | null;
  /** The home folder `homeDir()` resolves to. */
  home: string;
  /** Per command: replies used in order, the last one repeating. Beats the defaults. */
  responses: Record<string, Reply[]>;
}

export const ok = (value: unknown = null): Reply => ({ value });
export const hold = (): Reply => ({ hold: true });
/** The fake's own answer, e.g. after a failure: `respond(cmd, fail(...), pass())`. */
export const pass = (): Reply => ({ pass: true });
export function fail(
  kind: ErrorKind,
  message = `${kind} (from Rust)`,
  params?: Record<string, string>,
): Reply {
  return { error: { kind, message, ...(params ? { params } : {}) } };
}

/** A plugin's rejection: Tauri plugins reject with their error's message. */
export const failPlugin = (message: string): Reply => ({ error: message });

const NOW = Date.now();
const HOUR = 3_600_000;
/** An ISO date `n` hours from when the test file loaded (negative: in the past). */
export const hoursFromNow = (n: number) => new Date(NOW + n * HOUR).toISOString();
export const daysFromNow = (n: number) => hoursFromNow(n * 24);

/** The version the app reports (`app_info`), the one in package.json. */
export const VERSION: string = pkg.version;
export const HOME = "/Users/student";
export const LOGIN = "student";

export function settings(over: Partial<Settings> = {}): Settings {
  return {
    watchedFolders: [],
    scanEditorProjects: true,
    cloneFolder: `${HOME}/Classroom`,
    launchAtLogin: true,
    listFilter: "all",
    notifyHoursBefore: [24],
    defaultEditor: null,
    hidden: [],
    hiddenClasses: [],
    collapsedClasses: [],
    appearance: "system",
    language: "en",
    calendarFile: null,
    nativeCalendar: null,
    onboarded: true,
    commitActions: "none",
    updateChannel: "stable",
    // This version's notes were read: What's new doesn't take the main page's place.
    lastSeenVersion: VERSION,
    globalShortcut: null,
    ...over,
  };
}

export function tools(missing: ToolStatus["tool"][] = []): ToolStatus[] {
  const versions = { git: "2.50.1", gh: "2.83.0", "gh-student": "v1.4.0" } as const;
  return (["git", "gh", "gh-student"] as const).map((tool) => ({
    tool,
    installed: !missing.includes(tool),
    version: missing.includes(tool) ? null : versions[tool],
    outdated: false,
  }));
}

export function classroom(over: Partial<Classroom> = {}): Classroom {
  return {
    org: "cs50-school",
    slug: "cs101",
    name: "Intro to CS",
    term: "Fall 2026",
    active: true,
    secret: null,
    pagesBaseUrl: null,
    ...over,
  };
}

export function assignment(over: Partial<Assignment> & { slug: string }): Assignment {
  return {
    name: over.slug,
    mode: "individual",
    description: null,
    due: daysFromNow(3),
    availableFrom: null,
    submissionMode: "tag",
    locked: false,
    closed: false,
    feedbackPr: false,
    passThreshold: null,
    teamFormation: null,
    repoVisibility: null,
    renamedFrom: null,
    template: null,
    ...over,
  };
}

export function grade(score: number, maxScore: number, over: Partial<Grade> = {}): Grade {
  return {
    tag: "submit/20261010T120000Z-abc1234",
    score,
    maxScore,
    testsPassed: score === maxScore ? 3 : 2,
    testsTotal: 3,
    tests: [],
    publishedAt: hoursFromNow(-5),
    url: "https://github.com/cs50-school/release",
    releaseNotes: null,
    ...over,
  };
}

/** One test of a grade; failed, with no output, unless `over` says otherwise. */
export function gradeTest(name: string, over: Partial<GradeTest> = {}): GradeTest {
  return {
    name,
    passed: false,
    score: 0,
    maxScore: 1,
    output: null,
    outputClipped: false,
    timedOutAfter: null,
    ...over,
  };
}

export interface ProjectOptions {
  slug: string;
  name?: string;
  classroom?: Classroom;
  assignment?: Partial<Assignment>;
  /** null: not cloned. */
  local?: Partial<LocalState> | null;
  /** null: GitHub doesn't know it. */
  remote?: Partial<RemoteRepo> | null;
  pending?: Project["pending"];
  /** Rust's `alerts::needs_work`; by default something pending, or no clone. */
  needsWork?: boolean;
  lateSeconds?: number | null;
  group?: Project["group"];
  editor?: string | null;
}

/** A clone of an accepted assignment, on GitHub and on disk, nothing pending. */
export function project(options: ProjectOptions): Project {
  const cls = options.classroom ?? classroom();
  const repo = `${cls.slug}-${options.slug}-${LOGIN}`;
  const head = "a1b2c3d4e5f6";
  const a = assignment({
    slug: options.slug,
    name: options.name ?? options.slug,
    ...options.assignment,
  });
  const local: LocalState | null =
    options.local === null
      ? null
      : {
          branch: "main",
          head,
          ahead: 0,
          behind: 0,
          changedFiles: 0,
          changedPaths: [],
          trackedChanges: 0,
          unmerged: [],
          remoteAhead: false,
          ...options.local,
        };
  const remote: RemoteRepo | null =
    options.remote === null
      ? null
      : {
          owner: cls.org,
          name: repo,
          url: `https://github.com/${cls.org}/${repo}`,
          archived: false,
          canPush: true,
          defaultBranch: "main",
          head: { oid: head, at: hoursFromNow(-2), author: LOGIN },
          lastSubmission: null,
          config: { classroom: cls.slug, assignment: options.slug, secret: null },
          template: null,
          unmerged: [],
          grade: null,
          history: [],
          grading: null,
          feedback: null,
          ...options.remote,
        };
  return {
    id: `${cls.org}/${repo}`,
    owner: cls.org,
    repo,
    config: { classroom: cls.slug, assignment: options.slug, secret: null },
    path: local ? `${HOME}/Classroom/${repo}` : null,
    local,
    remote,
    classroom: cls,
    assignment: a,
    pending: options.pending ?? null,
    needsWork: options.needsWork ?? (options.pending != null || local === null),
    webUrl: `https://classroom50.org/${cls.org}/${cls.slug}/assignments/${options.slug}/submission`,
    lateSeconds: options.lateSeconds ?? null,
    editor: options.editor ?? null,
    group: options.group ?? null,
  };
}

/** A published assignment not accepted yet. */
export function unaccepted(
  options: { slug: string; name?: string; classroom?: Classroom } & Partial<Assignment>,
): Unaccepted {
  const { slug, name, classroom: cls = classroom(), ...rest } = options;
  return {
    id: `${cls.org}/${cls.slug}/${slug}`,
    classroom: cls,
    assignment: assignment({ slug, name: name ?? slug, ...rest }),
    acceptUrl: `https://classroom50.org/${cls.org}/${cls.slug}/assignments/${slug}/accept`,
    group: null,
  };
}

/** Self-update with nothing to do, on a bundle that updates itself. */
export function updateStatus(over: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    state: { phase: "idle" },
    deferred: false,
    installing: false,
    simulated: false,
    lastCheck: null,
    bundle: "app",
    canUpdate: true,
    installOnQuit: true,
    asksPassword: false,
    hasBeta: true,
    ...over,
  };
}

export function snapshot(over: Partial<Snapshot> = {}): Snapshot {
  return {
    projects: [],
    unaccepted: [],
    syncedAt: hoursFromNow(0),
    checkedAt: hoursFromNow(0),
    login: LOGIN,
    issue: null,
    ...over,
  };
}

/** A signed-in student with every tool, set up, and the given list. */
export function scenario(over: Partial<Scenario> = {}): Scenario {
  return {
    appInfo: { os: "macos", version: VERSION, nativeCalendar: true, globalShortcuts: true },
    settings: settings(),
    tools: tools(),
    auth: { loggedIn: true, login: LOGIN, missingScopes: [], envToken: null },
    snapshot: snapshot(),
    editors: [{ id: "vscode", name: "Visual Studio Code" }],
    accounts: [{ login: LOGIN, active: true }],
    systemLanguages: ["en-US"],
    unseen: [],
    syncDetails: {
      login: LOGIN,
      syncedAt: hoursFromNow(0),
      checkedAt: hoursFromNow(0),
      stats: {
        rateLimit: null,
        full: null,
        status: null,
        local: null,
        reposSeen: 0,
        scanRoots: 0,
        watchTargets: 0,
        phases: {},
      },
      rateLimit: { remaining: 4900, limit: 5000, resetAt: null },
      issue: null,
      assignments: 0,
      clones: 0,
    },
    update: updateStatus(),
    deepLink: null,
    home: HOME,
    responses: {},
    ...over,
  };
}
