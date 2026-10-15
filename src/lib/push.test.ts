import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Assignment, Project, PushCheck } from "$lib/api";
import { prompt } from "$lib/prompt.svelte";

const dialog = vi.hoisted(() => ({ ask: vi.fn(), message: vi.fn() }));
const api = vi.hoisted(() => ({
  pushCheck: vi.fn(),
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

const { commitPush, gradedOnPush } = await import("$lib/push");

const project = (assignment: Partial<Assignment> | null = {}) =>
  ({
    id: "o/r",
    assignment: assignment ? ({ mode: "individual", ...assignment } as Assignment) : null,
    remote: { defaultBranch: "main" },
  }) as Project;

const check = (over: Partial<PushCheck> = {}): PushCheck => ({
  files: ["a.txt"],
  untracked: [],
  large: [],
  tooLarge: [],
  ahead: 0,
  hasIdentity: true,
  ...over,
});

/** Answers the in-popover prompt once it opens. */
async function answer(text: string | null) {
  await vi.waitFor(() => expect(prompt.current).not.toBeNull());
  prompt.current?.resolve(text);
}

beforeEach(() => {
  vi.resetAllMocks();
  api.commitPush.mockResolvedValue({ commit: "abc" });
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
    api.pushCheck.mockResolvedValue(check());
    const done = commitPush(project(), "Lab");
    await answer("  Fix the loop ");
    expect(await done).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", "Fix the loop");
    expect(dialog.ask).not.toHaveBeenCalled();
  });

  it("does nothing when the prompt is cancelled", async () => {
    api.pushCheck.mockResolvedValue(check());
    const done = commitPush(project(), "Lab");
    await answer(null);
    expect(await done).toBe(false);
    expect(api.commitPush).not.toHaveBeenCalled();
  });

  it("confirms a late push first", async () => {
    api.pushCheck.mockResolvedValue(check());
    dialog.ask.mockResolvedValue(false);
    expect(await commitPush(project({ due: "2000-01-01T00:00:00Z" }), "Lab")).toBe(false);
    expect(prompt.current).toBeNull();
    expect(api.commitPush).not.toHaveBeenCalled();
  });

  it("only confirms when there are just commits to push", async () => {
    api.pushCheck.mockResolvedValue(check({ files: [], ahead: 2 }));
    dialog.ask.mockResolvedValue(true);
    expect(await commitPush(project(), "Lab")).toBe(true);
    expect(api.commitPush).toHaveBeenCalledWith("o/r", null);
  });

  it("sets the identity in the clone when git has none", async () => {
    api.pushCheck.mockResolvedValue(check({ hasIdentity: false }));
    api.suggestedGitIdentity.mockResolvedValue({ name: "Ada", email: "1+ada@x" });
    dialog.ask.mockResolvedValue(true);
    const done = commitPush(project(), "Lab");
    await answer("msg");
    expect(await done).toBe(true);
    expect(api.setGitIdentity).toHaveBeenCalledWith("o/r", "Ada", "1+ada@x");
  });

  it("refuses files GitHub would reject", async () => {
    api.pushCheck.mockResolvedValue(check({ tooLarge: ["big.bin"] }));
    expect(await commitPush(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("big.bin"),
      expect.anything(),
    );
  });

  it("words a refusal", async () => {
    api.pushCheck.mockRejectedValue({ kind: "remoteAhead", message: "x" });
    expect(await commitPush(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith(
      expect.stringContaining("Pull first"),
      expect.objectContaining({ kind: "warning" }),
    );
  });
});
