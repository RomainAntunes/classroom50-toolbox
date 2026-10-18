import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project } from "#lib/api.js";

const dialog = vi.hoisted(() => ({ message: vi.fn() }));
const api = vi.hoisted(() => ({ pullProject: vi.fn() }));
const announce = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("#lib/announce.svelte.js", () => ({ announce }));
vi.mock("#lib/api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/api.js")>()),
  api,
  withDialog: <T>(run: () => Promise<T>) => run(),
}));

const { pullProject } = await import("#lib/pull.js");

const project = (defaultBranch: string | null = "main") =>
  ({ id: "o/r", remote: defaultBranch ? { defaultBranch } : null }) as Project;

beforeEach(() => {
  vi.resetAllMocks();
});

describe("pullProject", () => {
  it("pulls, announces it and shows no dialog", async () => {
    api.pullProject.mockResolvedValue(undefined);
    expect(await pullProject(project(), "Lab")).toBe(true);
    expect(api.pullProject).toHaveBeenCalledExactlyOnceWith("o/r");
    expect(announce).toHaveBeenCalledExactlyOnceWith("Pulled: Lab is up to date with GitHub");
    expect(dialog.message).not.toHaveBeenCalled();
  });

  it.each([
    ["localChanges", "changes that aren't committed yet"],
    ["otherBranch", "Switch to main to pull"],
    ["diverged", "need to be merged"],
  ])("words a %s refusal, not Rust's message", async (kind, words) => {
    api.pullProject.mockRejectedValue({ kind, message: "from Rust" });
    expect(await pullProject(project(), "Lab")).toBe(false);
    const [text, options] = dialog.message.mock.calls[0];
    expect(text).toContain(words);
    expect(text).not.toContain("from Rust");
    expect(options).toEqual({ title: "Lab", kind: "warning" });
    expect(announce).not.toHaveBeenCalled();
  });

  it("names GitHub's default branch, else main", async () => {
    api.pullProject.mockRejectedValue({ kind: "otherBranch", message: "x" });
    await pullProject(project("trunk"), "Lab");
    expect(dialog.message.mock.calls[0][0]).toContain("Switch to trunk");
    await pullProject(project(null), "Lab");
    expect(dialog.message.mock.calls[1][0]).toContain("Switch to main");
  });

  it("shows Rust's words for any other failure", async () => {
    api.pullProject.mockRejectedValue({ kind: "commandFailed", message: "fatal: no network" });
    expect(await pullProject(project(), "Lab")).toBe(false);
    expect(dialog.message).toHaveBeenCalledWith("The pull failed: fatal: no network", {
      title: "Lab",
      kind: "warning",
    });
  });

  it("shows a thrown string as it is", async () => {
    api.pullProject.mockRejectedValue("boom");
    await pullProject(project(), "Lab");
    expect(dialog.message.mock.calls[0][0]).toBe("The pull failed: boom");
  });
});
