import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, Settings, SubmitCheck } from "#lib/api.js";
import { session } from "#lib/session.svelte.js";
import { settings } from "#lib/settings.svelte.js";

const dialog = vi.hoisted(() => ({ ask: vi.fn(), message: vi.fn() }));
const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
const api = vi.hoisted(() => ({
  submitCheck: vi.fn(),
  submitProject: vi.fn(),
  upgradeTool: vi.fn(),
}));
const pull = vi.hoisted(() => ({ pullProject: vi.fn() }));
const push = vi.hoisted(() => ({ commitLocal: vi.fn() }));
const announce = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("#lib/pull.js", () => pull);
vi.mock("#lib/push.js", () => push);
vi.mock("#lib/announce.svelte.js", () => ({ announce }));
vi.mock("#lib/api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/api.js")>()),
  api,
  withDialog: <T>(run: () => Promise<T>) => run(),
}));

const { submitProject } = await import("#lib/submit.js");

const HEAD = "a1b2c3";
type Over = { due?: string | null; local?: Partial<Project["local"]>; remote?: object };
const project = ({ due = null, local = {}, remote = {} }: Over = {}) =>
  ({
    id: "o/r",
    path: "/c/lab",
    assignment: { mode: "individual", due },
    local: { head: "local", remoteAhead: false, ...local },
    remote: { defaultBranch: "main", head: { oid: HEAD, author: null }, ...remote },
  }) as unknown as Project;

const clean: SubmitCheck = { hasMarker: true, uncommitted: [], editorFiles: [] };
const tooOld = {
  kind: "ghStudentOutdated",
  message: "too old",
  params: { version: "v1.52.2", min: "1.56.0" },
};

/** The text and options of the first `ask`. */
const asked = (n = 0) => dialog.ask.mock.calls[n] as [string, Record<string, unknown>];

beforeEach(() => {
  vi.resetAllMocks();
  settings.current = { commitActions: "none" } as Settings;
  api.submitCheck.mockResolvedValue(clean);
  api.submitProject.mockResolvedValue({ url: null });
});

describe("the confirm", () => {
  it("says where the work goes, and submits on Submit", async () => {
    dialog.ask.mockResolvedValue(true);
    expect(await submitProject(project(), "Lab")).toBe(true);
    const [text, options] = asked();
    expect(text).toBe(
      "This sends your committed work in /c/lab to main on GitHub as a new commit. Your local commits and branches stay as they are.",
    );
    expect(options).toMatchObject({ title: "Lab", kind: "info", okLabel: "Submit" });
    expect(api.submitProject).toHaveBeenCalledExactlyOnceWith("o/r");
    expect(announce).toHaveBeenCalledWith("Submitted: Lab");
  });

  it("warns when GitHub is ahead, naming who pushed when known", async () => {
    dialog.ask.mockResolvedValue(false);
    await submitProject(project({ local: { remoteAhead: true } }), "Lab");
    expect(asked()[0]).toContain("GitHub has commits this clone doesn't have. Submitting");
    expect(asked()[1].kind).toBe("warning");
    await submitProject(
      project({ local: { remoteAhead: true }, remote: { head: { oid: "x", author: "bob" } } }),
      "Lab",
    );
    expect(asked(1)[0]).toContain("(the latest by bob)");
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("points out the same work graded again", async () => {
    dialog.ask.mockResolvedValue(false);
    await submitProject(project({ local: { head: HEAD } }), "Lab");
    expect(asked()[0]).toContain("this grades the same work again");
  });

  it("says a late submission is marked late, and only then", async () => {
    dialog.ask.mockResolvedValue(false);
    await submitProject(project({ due: "2000-01-01T00:00:00Z" }), "Lab");
    expect(asked()[0]).toContain("will be marked late");
    await submitProject(project({ due: "2999-01-01T00:00:00Z" }), "Lab");
    expect(asked(1)[0]).not.toContain("late");
  });

  it("names the editor files sent along", async () => {
    api.submitCheck.mockResolvedValue({ ...clean, editorFiles: [".vscode/", ".DS_Store"] });
    dialog.ask.mockResolvedValue(false);
    await submitProject(project(), "Lab");
    expect(asked()[0]).toContain("sent too: .vscode/, .DS_Store.");
  });

  it("Cancel submits nothing and announces nothing", async () => {
    dialog.ask.mockResolvedValue(false);
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(api.submitProject).not.toHaveBeenCalled();
    expect(announce).not.toHaveBeenCalled();
    expect(dialog.message).not.toHaveBeenCalled();
  });
});

describe("after submitting", () => {
  it("only says it's done when there is no submission page", async () => {
    dialog.ask.mockResolvedValue(true);
    await submitProject(project(), "Lab");
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringMatching(/^Submitted\. The autograder runs now/),
      { title: "Lab", kind: "info" },
    );
    expect(opener.openUrl).not.toHaveBeenCalled();
  });

  it("offers the submission page, opened only on View submission", async () => {
    api.submitProject.mockResolvedValue({ url: "https://classroom50.org/s" });
    dialog.ask.mockResolvedValueOnce(true).mockResolvedValueOnce(true);
    expect(await submitProject(project(), "Lab")).toBe(true);
    expect(asked(1)[1]).toMatchObject({ okLabel: "View submission", cancelLabel: "Close" });
    expect(opener.openUrl).toHaveBeenCalledWith("https://classroom50.org/s");

    dialog.ask.mockResolvedValueOnce(true).mockResolvedValueOnce(false);
    expect(await submitProject(project(), "Lab")).toBe(true);
    expect(opener.openUrl).toHaveBeenCalledOnce();
  });
});

describe("failures", () => {
  it("shows Rust's words", async () => {
    dialog.ask.mockResolvedValue(true);
    api.submitProject.mockRejectedValue({ kind: "student", message: "submissions are closed" });
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith("The submission failed: submissions are closed", {
      title: "Lab",
      kind: "error",
    });
  });

  it("a renamed assignment offers Pull and never submits after it", async () => {
    api.submitCheck.mockRejectedValue({ kind: "staleMarker", message: "x" });
    dialog.ask.mockResolvedValue(true);
    const lab = project();
    expect(await submitProject(lab, "Lab")).toBe(false);
    expect(asked()[0]).toContain("Your teacher renamed this assignment");
    expect(asked()[1]).toMatchObject({ okLabel: "Pull", cancelLabel: "Cancel" });
    expect(pull.pullProject).toHaveBeenCalledExactlyOnceWith(lab, "Lab");
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("a renamed assignment, Pull declined: nothing runs", async () => {
    dialog.ask.mockResolvedValueOnce(true).mockResolvedValueOnce(false);
    api.submitProject.mockRejectedValue({ kind: "staleMarker", message: "x" });
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(pull.pullProject).not.toHaveBeenCalled();
    expect(dialog.message).not.toHaveBeenCalled();
  });
});

describe("uncommitted work", () => {
  const dirty = { ...clean, uncommitted: ["a.c"] };

  it("Commit… declined commits nothing", async () => {
    settings.current = { commitActions: "push" } as Settings;
    api.submitCheck.mockResolvedValue(dirty);
    dialog.ask.mockResolvedValue(false);
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(push.commitLocal).not.toHaveBeenCalled();
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("with no settings loaded, points to the editor", async () => {
    settings.current = null;
    api.submitCheck.mockResolvedValue(dirty);
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(dialog.message.mock.calls[0][0]).toContain("in your editor or a terminal");
  });
});

describe("updating an old gh-student", () => {
  beforeEach(() => {
    api.submitCheck.mockRejectedValueOnce(tooOld);
    session.tools = [];
  });

  it("asks with the versions, and stops when still too old", async () => {
    dialog.ask.mockResolvedValue(true);
    api.upgradeTool.mockResolvedValue({
      tool: "gh-student",
      installed: true,
      version: "v1.53.0",
      outdated: true,
    });
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(asked()[0]).toContain("needs version 1.56.0 or newer, and v1.52.2 is installed");
    expect(asked()[1]).toMatchObject({ okLabel: "Update" });
    expect(announce).toHaveBeenCalledWith("Classroom 50 (gh student) updated");
    // Not checked again: the update wasn't enough.
    expect(api.submitCheck).toHaveBeenCalledOnce();
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("updates the shared tool statuses", async () => {
    session.tools = [{ tool: "gh-student", installed: true, version: "v1.52.2", outdated: true }];
    dialog.ask.mockResolvedValueOnce(true).mockResolvedValueOnce(false);
    const next = { tool: "gh-student", installed: true, version: "v1.58.1", outdated: false };
    api.upgradeTool.mockResolvedValue(next);
    await submitProject(project(), "Lab");
    expect(session.tools).toEqual([next]);
  });

  it("a cancelled update says nothing", async () => {
    dialog.ask.mockResolvedValue(true);
    api.upgradeTool.mockRejectedValue({ kind: "cancelled", message: "x" });
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(api.upgradeTool).toHaveBeenCalledOnce();
    expect(dialog.message).not.toHaveBeenCalled();
  });

  it.each([
    ["notSignedIn", "Sign in to GitHub first.", false],
    ["busy", "already running", false],
    ["commandFailed", "network down", true],
  ])("a %s failure is worded, with the command only when it helps", async (kind, words, cmd) => {
    dialog.ask.mockResolvedValue(true);
    api.upgradeTool.mockRejectedValue({ kind, message: "network down" });
    expect(await submitProject(project(), "Lab")).toBe(false);
    const [text, options] = dialog.message.mock.calls[0];
    expect(text).toMatch(/^Couldn't update Classroom 50 \(gh student\)\./);
    expect(text).toContain(words);
    expect(text.includes("gh extension upgrade gh-student")).toBe(cmd);
    expect(options).toEqual({ title: "Lab", kind: "error" });
    expect(api.submitProject).not.toHaveBeenCalled();
  });
});
