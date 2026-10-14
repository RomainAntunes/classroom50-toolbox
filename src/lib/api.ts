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

export interface LocalProject {
  path: string;
  owner: string | null;
  repo: string | null;
  config: RepoConfig;
}

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

export interface Settings {
  watchedFolders: string[];
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
  scanProjects: () => invoke<LocalProject[]>("scan_projects"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  login: () => invoke<AuthStatus>("login"),
  cancelLogin: () => invoke<void>("cancel_login"),
  fetchAssignments: (org: string, classroom: string, secret: string | null) =>
    invoke<Assignment[]>("fetch_assignments", { org, classroom, secret }),
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
