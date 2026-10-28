import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorInfo, Project, Settings, Snapshot } from "#lib/api.js";
import { editors } from "#lib/editor.svelte.js";
import { setLocale } from "#lib/i18n/index.js";
import { session } from "#lib/session.svelte.js";
import { settings } from "#lib/settings.svelte.js";
import { handleTrayAction, sendTrayMenu, trayMenu, type TrayInput } from "#lib/tray.js";

const api = vi.hoisted(() => ({
  setTrayMenu: vi.fn(),
  openInEditor: vi.fn(),
}));
vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => ({ page: { url: new URL("http://localhost/") } }));
vi.mock("#lib/api.js", () => ({
  api,
  errorMessage: String,
  withDialog: (run: () => unknown) => run(),
}));

afterEach(() => setLocale("en"));

const idea: EditorInfo = { id: "idea", name: "IntelliJ IDEA" };
const vscode: EditorInfo = { id: "vscode", name: "Visual Studio Code" };
const now = new Date("2026-10-28T14:00:00Z");

interface Options {
  name?: string;
  classroom?: string;
  path?: string | null;
  activeAt?: string | null;
  editor?: string | null;
}

/** A clone of `slug` in class `classroom` (org `acme`), last worked in at `activeAt`. */
function clone(slug: string, options: Options = {}): Project {
  const classroom = options.classroom ?? "cs50";
  return {
    id: `acme/${classroom}-${slug}-alex`,
    owner: "acme",
    repo: `${classroom}-${slug}-alex`,
    config: { classroom, assignment: slug, secret: null },
    path: options.path === undefined ? `/code/${classroom}-${slug}` : options.path,
    local: { activeAt: options.activeAt ?? null },
    assignment: { name: options.name ?? slug },
    classroom: { name: classroom.toUpperCase() },
    editor: options.editor ?? null,
  } as Project;
}

function input(projects: Project[], over: Partial<TrayInput> = {}): TrayInput {
  return {
    snapshot: { projects, unaccepted: [] } as unknown as Snapshot,
    update: null,
    hidden: { ids: [], classes: [] },
    editor: true,
    ...over,
  };
}

const hour = (h: number, m = 0) => new Date(Date.UTC(2026, 9, 28, h, m)).toISOString();
const labels = (i: TrayInput) => trayMenu(i, now).recent.map((item) => item.label);

describe("recent", () => {
  it("lists clones worked in last first, never-touched ones last", () => {
    const menu = trayMenu(
      input([
        clone("hello", { activeAt: hour(9) }),
        clone("mario", { activeAt: null }),
        clone("cash", { activeAt: hour(13) }),
        clone("credit", { activeAt: hour(11) }),
      ]),
      now,
    );
    expect(menu.recent).toEqual([
      { id: "acme/cs50-cash-alex", label: "cash" },
      { id: "acme/cs50-credit-alex", label: "credit" },
      { id: "acme/cs50-hello-alex", label: "hello" },
      { id: "acme/cs50-mario-alex", label: "mario" },
    ]);
  });

  it("keeps five", () => {
    const many = Array.from({ length: 7 }, (_, i) => clone(`lab-${i}`, { activeAt: hour(i) }));
    expect(labels(input(many))).toEqual(["lab-6", "lab-5", "lab-4", "lab-3", "lab-2"]);
  });

  it("names the assignment, as the list does", () => {
    expect(labels(input([clone("hello", { name: "Hello, World" })]))).toEqual(["Hello, World"]);
  });

  it("leaves out projects without a clone", () => {
    const projects = [clone("hello", { path: null }), clone("mario")];
    expect(labels(input(projects))).toEqual(["mario"]);
  });

  it("leaves out hidden projects and hidden classes", () => {
    const projects = [
      clone("hello"),
      clone("mario"),
      clone("lab-1", { classroom: "algo" }),
      clone("lab-2", { classroom: "Algo" }),
    ];
    const hidden = { ids: ["acme/cs50-hello-alex"], classes: ["acme/algo"] };
    expect(labels(input(projects, { hidden }))).toEqual(["mario"]);
  });

  it("adds the class when two picks share a name", () => {
    const projects = [
      clone("lab-1", { name: "Lab 1", classroom: "algo", activeAt: hour(12) }),
      clone("lab-1", { name: "Lab 1", classroom: "web", activeAt: hour(11) }),
      clone("hello", { activeAt: hour(10) }),
    ];
    expect(labels(input(projects))).toEqual(["Lab 1 (ALGO)", "Lab 1 (WEB)", "hello"]);
    setLocale("ja");
    expect(labels(input(projects))[0]).toBe("Lab 1（ALGO）");
  });

  it("offers nothing without an editor to open them in", () => {
    expect(trayMenu(input([clone("hello")], { editor: false }), now).recent).toEqual([]);
  });

  it("offers nothing before the first snapshot", () => {
    expect(trayMenu(input([], { snapshot: null }), now).recent).toEqual([]);
  });

  it("sends nothing when a clone was worked in but the order holds", () => {
    api.setTrayMenu.mockReset().mockResolvedValue(null);
    const before = [clone("hello", { activeAt: hour(12) }), clone("mario", { activeAt: hour(9) })];
    sendTrayMenu(trayMenu(input(before), now));
    // A sync later: hello's index moved again, still the newest.
    const after = [clone("hello", { activeAt: hour(13) }), clone("mario", { activeAt: hour(9) })];
    sendTrayMenu(trayMenu(input(after), now));
    expect(api.setTrayMenu).toHaveBeenCalledOnce();
    // Mario worked in since: the order changes, so the menu does.
    const swapped = [
      clone("hello", { activeAt: hour(13) }),
      clone("mario", { activeAt: hour(13, 30) }),
    ];
    sendTrayMenu(trayMenu(input(swapped), now));
    expect(api.setTrayMenu).toHaveBeenCalledTimes(2);
  });
});

const snap = (projects: Project[]) => ({ projects, unaccepted: [] }) as unknown as Snapshot;

describe("handleTrayAction open", () => {
  beforeEach(() => {
    session.snapshot = null;
    api.openInEditor.mockReset().mockResolvedValue(null);
    editors.installed = [idea, vscode];
    settings.current = { defaultEditor: null } as Settings;
  });

  it("opens the clone in the editor that last opened it", async () => {
    session.snapshot = snap([clone("hello", { editor: "vscode" })]);
    await handleTrayAction({ kind: "open", id: "acme/cs50-hello-alex" });
    expect(api.openInEditor).toHaveBeenCalledWith("/code/cs50-hello", "vscode");
  });

  it("uses the default editor otherwise", async () => {
    settings.current = { defaultEditor: "vscode" } as Settings;
    session.snapshot = snap([clone("hello")]);
    await handleTrayAction({ kind: "open", id: "acme/cs50-hello-alex" });
    expect(api.openInEditor).toHaveBeenCalledWith("/code/cs50-hello", "vscode");
  });

  it("does nothing for a project gone or without a clone", async () => {
    session.snapshot = snap([clone("hello", { path: null })]);
    await handleTrayAction({ kind: "open", id: "acme/cs50-hello-alex" });
    await handleTrayAction({ kind: "open", id: "acme/cs50-mario-alex" });
    expect(api.openInEditor).not.toHaveBeenCalled();
  });

  it("does nothing once no editor is installed", async () => {
    editors.installed = [];
    session.snapshot = snap([clone("hello")]);
    await handleTrayAction({ kind: "open", id: "acme/cs50-hello-alex" });
    expect(api.openInEditor).not.toHaveBeenCalled();
  });
});
