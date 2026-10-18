// Runs in the page before the app boots (bundled by `bundle.ts`, injected as an init
// script): a fake Rust side built on Tauri's own IPC mocks. Every `invoke` is recorded,
// answered from the scenario (or a per-command reply the test set), and nothing reaches a
// real backend, so a flow under test can't write to GitHub by construction. Native dialogs
// and menus wait for the test to answer or pick, through `window.__E2E__`.
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { CommandError, Settings } from "../../../src/lib/api";
import type { Reply, Scenario } from "../fixtures";

export interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

/** A native dialog waiting for an answer (message/ask/confirm, or a file picker). */
export interface PendingDialog {
  type: "message" | "open" | "save";
  message: string | null;
  title: string | null;
  kind: string | null;
  /** The button labels by role (message dialogs). */
  labels: Record<string, string>;
  options: unknown;
}

export interface MenuNode {
  id: string;
  kind: string;
  text: string | null;
  enabled: boolean;
  checked: boolean | null;
  items: MenuNode[];
}

interface Handler {
  onmessage: (id: string) => void;
}

interface StoredItem {
  id: string;
  kind: string;
  options: Record<string, unknown>;
  handler: Handler | null;
  items: StoredItem[];
}

/** What a test reaches through `window.__E2E__` (see `app.ts` for the test-side helpers). */
export interface E2E {
  calls: Call[];
  unmocked: string[];
  notifications: { title: string; body?: string }[];
  readonly settings: Settings;
  emit(event: string, payload: unknown): Promise<unknown>;
  respond(cmd: string, replies: Reply[]): void;
  release(cmd: string, reply: Reply): void;
  pendingDialogs(): PendingDialog[];
  answerDialog(choice: string | null): void;
  menus(): MenuNode[];
  pickMenu(key: string): void;
}

declare global {
  interface Window {
    __E2E__: E2E;
  }
}

const STANDARD: Record<string, Record<string, string>> = {
  Ok: { ok: "Ok" },
  OkCancel: { ok: "Ok", cancel: "Cancel" },
  YesNo: { yes: "Yes", no: "No" },
  YesNoCancel: { yes: "Yes", no: "No", cancel: "Cancel" },
};

/** The dialog plugin's `buttons` (as sent to Rust) by role; Rust answers with a label. */
function buttonLabels(buttons: unknown): Record<string, string> {
  if (buttons === undefined || buttons === null) return { ok: "Ok" };
  if (typeof buttons === "string") return STANDARD[buttons] ?? { ok: "Ok" };
  const b = buttons as Record<string, unknown>;
  if (typeof b.OkCustom === "string") return { ok: b.OkCustom };
  if (Array.isArray(b.OkCancelCustom)) {
    const [ok, cancel] = b.OkCancelCustom as string[];
    return { ok, cancel };
  }
  if (Array.isArray(b.YesNoCancelCustom)) {
    const [yes, no, cancel] = b.YesNoCancelCustom as string[];
    return { yes, no, cancel };
  }
  return { ok: "Ok" };
}

/** JSON-safe copy of invoke args (channels become their id string). */
function plain(value: unknown): Record<string, unknown> {
  if (value === undefined || value === null) return {};
  return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

export function install(scenario: Scenario): E2E {
  const state = structuredClone(scenario);
  const calls: Call[] = [];
  const unmocked: string[] = [];
  const notifications: { title: string; body?: string }[] = [];
  const dialogs: (PendingDialog & { resolve: (value: unknown) => void })[] = [];
  const held = new Map<string, { resolve: (v: unknown) => void; reject: (e: unknown) => void }[]>();
  const items = new Map<number, StoredItem>();
  let nextRid = 1;
  let popups: StoredItem[] = [];

  function storeItem(kind: string, options: Record<string, unknown>, handler: Handler | null) {
    const rid = nextRid++;
    const id = typeof options.id === "string" ? options.id : `item-${rid}`;
    const children = Array.isArray(options.items)
      ? (options.items as unknown[]).map((child) => {
          // An item made earlier comes as `[rid, kind]`; an inline one as its options.
          if (Array.isArray(child)) return items.get(child[0] as number)!;
          const inline = child as Record<string, unknown>;
          const kindOf =
            "item" in inline ? "Predefined" : "checked" in inline ? "Check" : "MenuItem";
          return storeItem(kindOf, inline, (inline.handler as Handler) ?? null).item;
        })
      : [];
    const item: StoredItem = { id, kind, options, handler, items: children };
    items.set(rid, item);
    return { rid, item };
  }

  function describe(item: StoredItem): MenuNode {
    const o = item.options;
    const predefined = o.item as string | Record<string, unknown> | undefined;
    return {
      id: item.id,
      kind: item.kind,
      text:
        typeof o.text === "string" ? o.text : typeof predefined === "string" ? predefined : null,
      enabled: o.enabled !== false,
      checked: typeof o.checked === "boolean" ? o.checked : null,
      items: item.items.map(describe),
    };
  }

  function findItem(list: StoredItem[], key: string): StoredItem | null {
    for (const item of list) {
      if (item.id === key || item.options.text === key) return item;
      const inner = findItem(item.items, key);
      if (inner) return inner;
    }
    return null;
  }

  function dialog(entry: PendingDialog): Promise<unknown> {
    return new Promise((resolve) => dialogs.push({ ...entry, resolve }));
  }

  /** What the real Rust side would answer, from the scenario. */
  function fallback(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "app_info":
        return state.appInfo;
      case "get_settings":
        return state.settings;
      case "update_settings":
        state.settings = { ...(args.next as Settings) };
        return state.settings;
      case "tool_statuses":
        return state.tools;
      case "tool_jobs":
        return [];
      case "tool_update":
        return null;
      case "update_status":
      case "check_update":
        return state.update;
      case "defer_update":
        state.update = { ...state.update, deferred: true };
        void emit("update-state", state.update);
        return null;
      case "install_update":
        state.update = { ...state.update, installing: true };
        void emit("update-state", state.update);
        return null;
      case "export_calendar": {
        // Rust writes the file and keeps it updated from then on (`calendarFile`).
        state.settings = { ...state.settings, calendarFile: args.path as string };
        const { projects, unaccepted } = state.snapshot;
        return [...projects, ...unaccepted].filter((item) => item.assignment?.due).length;
      }
      case "active_global_shortcut":
        return state.settings.globalShortcut;
      case "simulated_network":
        return "normal";
      case "install_command":
        return {
          program: "brew",
          args: ["install", args.tool],
          manual: `brew install ${args.tool}`,
        };
      case "auth_status":
        return state.auth;
      case "list_projects":
      case "refresh_projects":
      case "rescan_local":
        return state.snapshot;
      case "sync_progress":
        return { running: false, foreground: false, fraction: 0 };
      case "sync_details":
        return state.syncDetails;
      case "unseen":
        return state.unseen;
      case "system_languages":
        return state.systemLanguages;
      case "installed_editors":
        return state.editors;
      case "list_accounts":
        return state.accounts;
      case "account_avatar":
        throw { kind: "offline", message: "no avatar in tests" } satisfies CommandError;
      case "git_protocol":
        return "https";
      case "take_deep_link": {
        const link = state.deepLink;
        state.deepLink = null;
        return link;
      }
      case "deep_link_done":
      case "open_calendar_file":
      case "set_popover_pinned":
      case "hide_popover":
      case "set_calendar_labels":
      case "set_tray_labels":
      case "cancel_login":
      case "cancel_tool_job":
      case "open_in_editor":
      case "open_terminal":
      case "set_git_identity":
      case "pause_global_shortcut":
      case "quit_app":
      case "simulate_update":
      case "simulate_network":
        return null;
      case "plugin:path|resolve_directory":
        return state.home;
      case "plugin:dialog|message":
        return dialog({
          type: "message",
          message: (args.message as string) ?? null,
          title: (args.title as string) ?? null,
          kind: (args.kind as string) ?? null,
          labels: buttonLabels(args.buttons),
          options: null,
        });
      case "plugin:dialog|open":
      case "plugin:dialog|save": {
        const options = (args.options ?? {}) as Record<string, unknown>;
        return dialog({
          type: cmd === "plugin:dialog|open" ? "open" : "save",
          message: null,
          title: (options.title as string) ?? null,
          kind: null,
          labels: {},
          options,
        });
      }
      case "plugin:menu|new": {
        const options = (args.options ?? {}) as Record<string, unknown>;
        const { rid, item } = storeItem(String(args.kind), options, args.handler as Handler);
        return [rid, item.id];
      }
      case "plugin:menu|popup": {
        const menu = items.get(args.rid as number);
        if (menu) popups.push(menu);
        return null;
      }
      case "plugin:notification|is_permission_granted":
        return true;
    }
    if (
      cmd.startsWith("plugin:menu|") ||
      cmd.startsWith("plugin:resources|") ||
      cmd.startsWith("plugin:opener|") ||
      cmd.startsWith("plugin:clipboard-manager|") ||
      cmd.startsWith("plugin:log|") ||
      cmd.startsWith("plugin:notification|") ||
      cmd.startsWith("plugin:process|")
    ) {
      return null;
    }
    unmocked.push(cmd);
    // Not a kind Rust has: the test fails on it (`unmocked`), whatever the UI does with it.
    throw { kind: "unmocked", message: `No e2e mock for ${cmd}` };
  }

  function answer(cmd: string, args: Record<string, unknown>): unknown {
    const replies = state.responses[cmd];
    if (replies?.length) {
      const reply: Reply = replies.length > 1 ? replies.shift()! : replies[0];
      if ("pass" in reply) return fallback(cmd, args);
      if ("error" in reply) throw reply.error;
      if ("hold" in reply) {
        return new Promise((resolve, reject) => {
          held.set(cmd, [...(held.get(cmd) ?? []), { resolve, reject }]);
        });
      }
      return reply.value;
    }
    return fallback(cmd, args);
  }

  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      const recorded = plain(args);
      calls.push({ cmd, args: recorded });
      return answer(cmd, (args ?? {}) as Record<string, unknown>);
    },
    { shouldMockEvents: true },
  );

  // The notification plugin goes through the web API: record instead of showing.
  class FakeNotification {
    static permission = "granted";
    static requestPermission = async () => "granted";
    constructor(title: string, options?: { body?: string }) {
      notifications.push({ title, body: options?.body });
    }
  }
  Object.defineProperty(window, "Notification", { value: FakeNotification, configurable: true });

  const api: E2E = {
    calls,
    unmocked,
    notifications,
    get settings() {
      return state.settings;
    },
    /** Sends a Rust event to the UI's listeners. */
    emit(event: string, payload: unknown) {
      return emit(event, payload);
    },
    /** Replaces how `cmd` answers from now on. */
    respond(cmd: string, replies: Reply[]) {
      state.responses[cmd] = [...replies];
    },
    /** Settles the oldest call of `cmd` that got a `hold` reply. */
    release(cmd: string, reply: Reply) {
      const waiting = held.get(cmd)?.shift();
      if (!waiting) throw new Error(`No held call of ${cmd}`);
      if ("error" in reply) waiting.reject(reply.error);
      else waiting.resolve("value" in reply ? reply.value : null);
    },
    pendingDialogs(): PendingDialog[] {
      return dialogs.map(({ resolve: _, ...rest }) => rest);
    },
    /** Answers the oldest dialog: a role (`ok`, `cancel`, `yes`, `no`) or a label for
     *  message dialogs, a path (or null) for pickers. */
    answerDialog(choice: string | null) {
      const pending = dialogs.shift();
      if (!pending) throw new Error("No dialog is open");
      if (pending.type !== "message") return pending.resolve(choice);
      // Escape and the window's close button resolve to something that isn't a label.
      pending.resolve(choice === null ? null : (pending.labels[choice] ?? choice));
    },
    /** Every native menu popped up so far, oldest first. */
    menus(): MenuNode[] {
      return popups.map(describe);
    },
    /** Clicks an item (by id or text) of the last menu popped up; the menu is gone after. */
    pickMenu(key: string) {
      const menu = popups[popups.length - 1];
      if (!menu) throw new Error("No menu is open");
      const item = findItem(menu.items, key);
      if (!item) {
        const texts = describe(menu).items.map((i) => i.text);
        throw new Error(`No menu item "${key}" in ${JSON.stringify(texts)}`);
      }
      if (item.options.enabled === false) throw new Error(`Menu item "${key}" is disabled`);
      if (!item.handler) throw new Error(`Menu item "${key}" has no action`);
      popups = [];
      item.handler.onmessage(item.id);
    },
  };
  window.__E2E__ = api;
  return api;
}
