import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Assignment, CommitCheck, Project } from "$lib/api";
import { prompt } from "$lib/prompt.svelte";

const dialog = vi.hoisted(() => ({ ask: vi.fn(), message: vi.fn() }));
const api = vi.hoisted(() => ({
  pushCheck: vi.fn(),
  commitCheck: vi.fn(),
  commitOnly: vi.fn(),
  submitCheck: vi.fn(),
  commitPush: vi.fn(),
  suggestedGitIdentity: vi.fn(),
  setGitIdentity: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("$lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/api")>()),
  api,
  withDialog: <T>(run: () => Promise<T>) => run(),
}));

const { commitLocal, commitPush, gradedOnPush } = await import("$lib/push");
const { submitProject } = await import("$lib/submit");

const project = (assignment: Partial<Assignment> | null = {}) =>
  ({
    id: "o/r",
    assignment: assignment ? ({ mode: "individual", ...assignment } as Assignment) : null,
    remote: { defaultBranch: "main" },
  }) as Project;

const check = (over: Partial<CommitCheck> = {}): CommitCheck => ({
  files: ["a.txt"],
  untracked: [],
  large: [],
  tooLarge: [],
  ahead: 0,
  hasIdentity: true,
  pushBlocked: null,
  ...over,
});

/** Answers the in-popover prompt once it opens, with its second action when `alt`. */
async function answer(text: string | null, alt = false) {
  await vi.waitFor(() => expect(prompt.current).not.toBeNull());
  prompt.current?.resolve(text, alt);
}

beforeEach(() => {
  vi.resetAllMocks();
  api.commitPush.mockResolvedValue({ commit: "abc" });
  api.commitOnly.mockResolvedValue({ commit: "abc" });
});

describe("gradedOnPush", () => {
  it("is every-push assignments only", () => {
    expect(gradedOnPush(project())).toBe(true);
    expect(gradedOnPush(project({ submissionMode: "every-push" }))).toBe(true);
    expect(gradedOnPush(project({ submissionMode: "tag" }))).toBe(false);
    expect(gradedOnPush(project(null))).toBe(false);
  });
});

describe("commitPush", () => {
  it("commits with the message typed", async () => {
    api.commitCheck.mockResolvedValue(check());
    const done = commitPush(project(), "Lab");
    await answer("  Fix the loop ");
    expect(await done).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", "Fix the loop");
    expect(dialog.ask).not.toHaveBeenCalled();
  });

  it("does nothing when the prompt is cancelled", async () => {
    api.commitCheck.mockResolvedValue(check());
    const done = commitPush(project(), "Lab");
    await answer(null);
    expect(await done).toBe(false);
    expect(api.commitPush).not.toHaveBeenCalled();
  });

  it("confirms a late push", async () => {
    api.commitCheck.mockResolvedValue(check());
    dialog.ask.mockResolvedValue(false);
    const done = commitPush(project({ due: "2000-01-01T00:00:00Z" }), "Lab");
    await answer("msg");
    expect(await done).toBe(false);
    expect(dialog.ask).toHaveBeenCalledOnce();
    expect(api.commitPush).not.toHaveBeenCalled();
  });

  it("commits only with the second action, late or not, without a confirm", async () => {
    api.commitCheck.mockResolvedValue(check());
    const onChoice = vi.fn();
    const done = commitPush(project({ due: "2000-01-01T00:00:00Z" }), "Lab", onChoice);
    await vi.waitFor(() => expect(prompt.current).not.toBeNull());
    expect(onChoice).not.toHaveBeenCalled();
    expect(prompt.current?.okLabel).toBe("Commit & push");
    expect(prompt.current?.altLabel).toBe("Commit only");
    await answer("WIP", true);
    expect(await done).toBe(true);
    expect(onChoice).toHaveBeenCalledExactlyOnceWith(false);
    expect(api.commitOnly).toHaveBeenCalledWith("o/r", "WIP");
    expect(api.commitPush).not.toHaveBeenCalled();
    expect(dialog.ask).not.toHaveBeenCalled();
  });

  it("only confirms when there are just commits to push", async () => {
    api.commitCheck.mockRejectedValue({ kind: "nothingToCommit", message: "x" });
    api.pushCheck.mockResolvedValue(check({ files: [], ahead: 2 }));
    dialog.ask.mockResolvedValue(true);
    expect(await commitPush(project(), "Lab")).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", null);
  });

  it("sets the identity in the clone when git has none", async () => {
    api.commitCheck.mockResolvedValue(check({ hasIdentity: false }));
    api.suggestedGitIdentity.mockResolvedValue({ name: "Ada", email: "1+ada@x" });
    dialog.ask.mockResolvedValue(true);
    const done = commitPush(project(), "Lab");
    await answer("msg");
    expect(await done).toBe(true);
    expect(api.setGitIdentity).toHaveBeenCalledWith("o/r", "Ada", "1+ada@x");
  });

  it("refuses files GitHub would reject", async () => {
    api.commitCheck.mockResolvedValue(check({ tooLarge: ["big.bin"] }));
    expect(await commitPush(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("big.bin"),
      expect.anything(),
    );
  });

  it("words a refusal", async () => {
    api.commitCheck.mockRejectedValue({ kind: "nothingToCommit", message: "x" });
    api.pushCheck.mockRejectedValue({ kind: "remoteAhead", message: "x" });
    expect(await commitPush(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("Pull first"),
      expect.objectContaining({ kind: "warning" }),
    );
  });
});

describe("commitLocal", () => {
  it("puts Commit first and offers Commit & push second", async () => {
    api.commitCheck.mockResolvedValue(check());
    const done = commitLocal(project(), "Lab");
    await vi.waitFor(() => expect(prompt.current).not.toBeNull());
    expect(prompt.current?.okLabel).toBe("Commit");
    expect(prompt.current?.altLabel).toBe("Commit & push");
    await answer("Fix");
    expect(await done).toBe(true);
    expect(api.commitOnly).toHaveBeenCalledWith("o/r", "Fix");
    expect(api.commitPush).not.toHaveBeenCalled();
  });

  it("pushes with the second action", async () => {
    api.commitCheck.mockResolvedValue(check());
    const done = commitLocal(project(), "Lab");
    await answer("Fix", true);
    expect(await done).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", "Fix");
  });

  it.each([
    ["tagMode", "graded through Submit"],
    ["otherBranch", "main, the graded branch"],
    ["remoteAhead", "GitHub has newer commits"],
    ["somethingElse", "push isn't available"],
  ])("offers only Commit when push is blocked (%s), and says why", async (reason, why) => {
    api.commitCheck.mockResolvedValue(check({ pushBlocked: reason }));
    const done = commitPush(project({ due: "2000-01-01T00:00:00Z" }), "Lab");
    await vi.waitFor(() => expect(prompt.current).not.toBeNull());
    expect(prompt.current?.okLabel).toBe("Commit");
    expect(prompt.current?.altLabel).toBeUndefined();
    expect(prompt.current?.message).toContain(why);
    await answer("Fix");
    expect(await done).toBe(true);
    expect(api.commitOnly).toHaveBeenCalledWith("o/r", "Fix");
    expect(dialog.ask).not.toHaveBeenCalled();
  });

  it("says when there is nothing to commit", async () => {
    api.commitCheck.mockRejectedValue({ kind: "nothingToCommit", message: "x" });
    expect(await commitLocal(project(), "Lab")).toBe(false);
    expect(api.pushCheck).not.toHaveBeenCalled();
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("Nothing to commit"),
      expect.anything(),
    );
  });

  it("words a failed commit as a commit", async () => {
    api.commitCheck.mockResolvedValue(check());
    api.commitOnly.mockRejectedValue({ kind: "unsupported", message: "hook failed" });
    const done = commitLocal(project(), "Lab");
    await answer("Fix");
    expect(await done).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith("Commit failed: hook failed", expect.anything());
  });
});

describe("submitProject without a marker", () => {
  const noMarker = { hasMarker: false, untracked: [], deleted: [] };

  it("offers Commit & push on every-push assignments", async () => {
    api.submitCheck.mockResolvedValue(noMarker);
    api.commitCheck.mockResolvedValue(check());
    dialog.ask.mockResolvedValue(true);
    const done = submitProject(project(), "Lab");
    await answer("First version");
    expect(await done).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", "First version");
  });

  it("only explains on tag-mode assignments", async () => {
    api.submitCheck.mockResolvedValue(noMarker);
    expect(await submitProject(project({ submissionMode: "tag" }), "Lab")).toBe(false);
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(dialog.message).toHaveBeenCalledOnce();
    expect(api.pushCheck).not.toHaveBeenCalled();
  });
});
