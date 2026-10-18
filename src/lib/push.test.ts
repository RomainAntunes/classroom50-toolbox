import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Assignment, CommitCheck, Project, Settings } from "#lib/api.js";
import { prompt } from "#lib/prompt.svelte.js";
import { settings } from "#lib/settings.svelte.js";

const dialog = vi.hoisted(() => ({ ask: vi.fn(), message: vi.fn() }));
const api = vi.hoisted(() => ({
  pushCheck: vi.fn(),
  commitCheck: vi.fn(),
  commitOnly: vi.fn(),
  submitCheck: vi.fn(),
  submitProject: vi.fn(),
  upgradeTool: vi.fn(),
  commitPush: vi.fn(),
  suggestedGitIdentity: vi.fn(),
  setGitIdentity: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("#lib/api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/api.js")>()),
  api,
  withDialog: <T>(run: () => Promise<T>) => run(),
}));

const { commitLocal, commitPush, gradedOnPush } = await import("#lib/push.js");
const { submitProject } = await import("#lib/submit.js");

const project = (assignment: Partial<Assignment> | null = {}) =>
  ({
    id: "o/r",
    assignment: assignment ? ({ mode: "individual", ...assignment } as Assignment) : null,
    remote: { defaultBranch: "main" },
  }) as Project;

const check = (over: Partial<CommitCheck> = {}): CommitCheck => ({
  files: ["a.txt"],
  untracked: [],
  skipped: [],
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

  it("names the editor and system files it leaves out", async () => {
    api.commitCheck.mockResolvedValue(check({ skipped: [".idea/", ".DS_Store"] }));
    const done = commitLocal(project(), "Lab");
    await vi.waitFor(() => expect(prompt.current).not.toBeNull());
    expect(prompt.current?.message).toContain(
      "Left out (editor and system files): .idea/, .DS_Store.",
    );
    await answer(null);
    expect(await done).toBe(false);
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
  const noMarker = { hasMarker: false, uncommitted: [], editorFiles: [] };
  const withActions = (commitActions: Settings["commitActions"]) =>
    (settings.current = { commitActions } as Settings);
  const tooOld = {
    kind: "ghStudentOutdated",
    message: "too old",
    params: { version: "v1.52.2", min: "1.56.0" },
  };

  it("submits like any repo, every-push or not, without offering Commit & push", async () => {
    withActions("push");
    for (const assignment of [{}, { submissionMode: "tag" as const }]) {
      api.submitCheck.mockResolvedValue(noMarker);
      api.submitProject.mockResolvedValue({ url: null });
      dialog.ask.mockResolvedValue(true);
      expect(await submitProject(project(assignment), "Lab")).toBe(true);
      expect(dialog.ask).toHaveBeenCalledWith(
        expect.stringContaining("This sends your committed work"),
        expect.anything(),
      );
      expect(api.submitProject).toHaveBeenCalledWith("o/r");
      expect(api.commitCheck).not.toHaveBeenCalled();
    }
  });

  it("only explains when the app doesn't know the assignment", async () => {
    api.submitCheck.mockRejectedValue({ kind: "unknownAssignment", message: "unknown" });
    expect(await submitProject(project(null), "Lab")).toBe(false);
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("Commit and push your work with git instead"),
      expect.anything(),
    );
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("offers to update an old gh-student, then checks again", async () => {
    api.submitCheck.mockRejectedValueOnce(tooOld).mockResolvedValue(noMarker);
    api.upgradeTool.mockResolvedValue({
      tool: "gh-student",
      installed: true,
      version: "v1.58.1",
      outdated: false,
    });
    // Update, then cancel the submit confirm.
    dialog.ask.mockResolvedValueOnce(true).mockResolvedValueOnce(false);
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(dialog.ask.mock.calls[0][0]).toContain("v1.52.2 is installed");
    expect(api.upgradeTool).toHaveBeenCalledWith("gh-student");
    expect(api.submitCheck).toHaveBeenCalledTimes(2);
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("updates nothing when the student declines", async () => {
    api.submitCheck.mockRejectedValue(tooOld);
    dialog.ask.mockResolvedValue(false);
    expect(await submitProject(project(), "Lab")).toBe(false);
    expect(api.upgradeTool).not.toHaveBeenCalled();
    expect(api.submitProject).not.toHaveBeenCalled();
  });
});

describe("submitProject with uncommitted changes", () => {
  const dirty = { hasMarker: true, uncommitted: ["a.c", "b.c"], editorFiles: [] };
  const withActions = (commitActions: Settings["commitActions"]) =>
    (settings.current = { commitActions } as Settings);
  const tagMode = () => ({ ...project({ submissionMode: "tag" }), path: "/c" }) as Project;

  it("points to the editor when the app doesn't commit", async () => {
    withActions("none");
    api.submitCheck.mockResolvedValue(dirty);
    expect(await submitProject(tagMode(), "Lab")).toBe(false);
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringMatching(/aren't committed yet: a\.c, b\.c\.[^]*editor or a terminal/),
      expect.anything(),
    );
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("offers Commit, then leaves Submit for later", async () => {
    withActions("commit");
    api.submitCheck.mockResolvedValue(dirty);
    api.commitCheck.mockResolvedValue(check({ pushBlocked: "tagMode" }));
    dialog.ask.mockResolvedValue(true);
    const done = submitProject(tagMode(), "Lab");
    await answer("Finish");
    expect(await done).toBe(false);
    expect(dialog.ask).toHaveBeenCalledWith(
      expect.stringContaining("Commit them first, then submit."),
      expect.objectContaining({ okLabel: "Commit…" }),
    );
    expect(api.commitOnly).toHaveBeenCalledWith("o/r", "Finish");
    expect(api.submitProject).not.toHaveBeenCalled();
  });

  it("submits a clean clone and names the editor files sent with it", async () => {
    api.submitCheck.mockResolvedValue({
      hasMarker: true,
      uncommitted: [],
      editorFiles: [".idea/"],
    });
    api.submitProject.mockResolvedValue({ url: null });
    dialog.ask.mockResolvedValue(true);
    expect(await submitProject(tagMode(), "Lab")).toBe(true);
    expect(dialog.ask).toHaveBeenCalledWith(
      expect.stringContaining("sent too: .idea/."),
      expect.anything(),
    );
    expect(api.submitProject).toHaveBeenCalledWith("o/r");
  });
});
