import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  DeepLink,
  DeepLinkTarget,
  Project,
  Snapshot,
  ToolStatus,
  Unaccepted,
} from "#lib/api.js";
import { session } from "#lib/session.svelte.js";

const dialog = vi.hoisted(() => ({ ask: vi.fn(), message: vi.fn() }));
const nav = vi.hoisted(() => ({ goto: vi.fn() }));
const api = vi.hoisted(() => ({
  takeDeepLink: vi.fn(),
  deepLinkDone: vi.fn(),
  fetchLinkedAssignment: vi.fn(),
  toolStatuses: vi.fn(),
  authStatus: vi.fn(),
  listProjects: vi.fn(),
}));
const accept = vi.hoisted(() => ({ acceptAssignment: vi.fn() }));
const clone = vi.hoisted(() => ({ cloneProject: vi.fn() }));
const loadSettings = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("$app/navigation", () => nav);
vi.mock("#lib/clone.js", () => clone);
vi.mock("#lib/accept.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/accept.js")>()),
  acceptAssignment: accept.acceptAssignment,
}));
vi.mock("#lib/settings.svelte.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/settings.svelte.js")>()),
  loadSettings,
}));
vi.mock("#lib/api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/api.js")>()),
  api,
  withDialog: <T>(run: () => Promise<T>) => run(),
}));

const { handleDeepLink, linkActive } = await import("#lib/deeplink.js");

const installed = (missing: string[] = []): ToolStatus[] =>
  (["git", "gh", "gh-student"] as const).map((tool) => ({
    tool,
    installed: !missing.includes(tool),
    version: "1",
    outdated: false,
  }));

const acceptLink: DeepLink = {
  kind: "accept",
  org: "cs50",
  classroom: "cs101",
  assignment: "lab3",
  key: null,
};
const openLink: DeepLink = { kind: "open", owner: "cs50", repo: "cs101-lab1-me" };

const item = (over: Partial<Unaccepted["assignment"]> = {}) =>
  ({
    id: "cs50/cs101/lab3",
    assignment: { slug: "lab3", name: "Lab 3", locked: false, availableFrom: null, ...over },
  }) as Unaccepted;

const lab1 = (path: string | null) =>
  ({
    id: "cs50/cs101-lab1-me",
    owner: "cs50",
    repo: "cs101-lab1-me",
    path,
    assignment: { name: "Lab 1" },
  }) as Project;

/** Runs one link through `handleDeepLink`. */
async function handle(target: DeepLinkTarget | null) {
  api.takeDeepLink.mockResolvedValueOnce(target).mockResolvedValue(null);
  await handleDeepLink();
}

beforeEach(() => {
  vi.resetAllMocks();
  loadSettings.mockResolvedValue({ onboarded: true });
  api.toolStatuses.mockResolvedValue(installed());
  api.authStatus.mockResolvedValue({ loggedIn: true, login: "me" });
  session.snapshot = { projects: [lab1(null)], unaccepted: [] } as unknown as Snapshot;
  session.focus = null;
});

describe("handleDeepLink", () => {
  it("does nothing without a link, and says nothing is done", async () => {
    await handle(null);
    expect(api.deepLinkDone).not.toHaveBeenCalled();
    expect(dialog.message).not.toHaveBeenCalled();
  });

  it("before the guide is done, only says so", async () => {
    loadSettings.mockResolvedValue({ onboarded: false });
    await handle({ kind: "accept", item: item(), listed: true });
    expect(dialog.message.mock.calls[0][0]).toContain("Finish setting up the app first");
    expect(api.toolStatuses).not.toHaveBeenCalled();
    expect(accept.acceptAssignment).not.toHaveBeenCalled();
    expect(api.deepLinkDone).toHaveBeenCalledOnce();
  });

  it("accepting needs gh-student; opening only gh", async () => {
    api.toolStatuses.mockResolvedValue(installed(["gh-student"]));
    await handle({ kind: "accept", item: item(), listed: true });
    expect(dialog.message.mock.calls[0]).toEqual([
      expect.stringContaining("needs the tools listed under Setup"),
      expect.objectContaining({ kind: "warning" }),
    ]);
    expect(nav.goto).toHaveBeenCalledWith("/");
    expect(accept.acceptAssignment).not.toHaveBeenCalled();

    dialog.message.mockClear();
    await handle({ kind: "project", id: lab1(null).id, link: openLink });
    expect(dialog.message).not.toHaveBeenCalled();
    expect(clone.cloneProject).toHaveBeenCalledOnce();
  });

  it("signed out, asks to sign in first", async () => {
    api.authStatus.mockResolvedValue({ loggedIn: false, login: null });
    await handle({ kind: "project", id: lab1(null).id, link: openLink });
    expect(dialog.message.mock.calls[0][0]).toContain("Sign in to GitHub first");
    expect(clone.cloneProject).not.toHaveBeenCalled();
    expect(session.auth).toEqual({ loggedIn: false, login: null });
  });

  it("an open link focuses the row and offers to clone a project not on disk", async () => {
    await handle({ kind: "project", id: lab1(null).id, link: openLink });
    expect(session.focus).toMatchObject({ id: lab1(null).id });
    const [id, mode, question] = clone.cloneProject.mock.calls[0];
    expect([id, mode]).toEqual([lab1(null).id, "ask"]);
    expect(question.title).toBe("Lab 1");
    expect(question.cancelLabel).toBe("Later");
    expect(question.message(null)).toBe(
      "cs50/cs101-lab1-me isn't on this computer yet. Clone it now?",
    );
    expect(question.message("~/C/x")).toContain("Clone it now into ~/C/x?");
    expect(api.deepLinkDone).toHaveBeenCalledOnce();
  });

  it("an open link to a clone only shows it", async () => {
    session.snapshot = { projects: [lab1("/c/lab1")], unaccepted: [] } as unknown as Snapshot;
    await handle({ kind: "project", id: lab1(null).id, link: openLink });
    expect(session.focus).toMatchObject({ id: lab1(null).id });
    expect(clone.cloneProject).not.toHaveBeenCalled();
  });

  it("reads the list first when the page never loaded it", async () => {
    session.snapshot = null;
    api.listProjects.mockResolvedValue({ projects: [lab1("/c")], unaccepted: [] });
    await handle({ kind: "project", id: lab1(null).id, link: openLink });
    expect(api.listProjects).toHaveBeenCalledOnce();
    expect(clone.cloneProject).not.toHaveBeenCalled();
  });

  it("accepts a listed assignment through the usual flow", async () => {
    const lab3 = item();
    await handle({ kind: "accept", item: lab3, listed: true });
    expect(session.focus).toMatchObject({ id: lab3.id });
    expect(accept.acceptAssignment).toHaveBeenCalledExactlyOnceWith(lab3);
  });

  it("a locked or upcoming assignment is refused with the row's hint", async () => {
    await handle({ kind: "accept", item: item({ locked: true }), listed: false });
    expect(dialog.message.mock.calls[0][0]).toMatch(/^Lab 3: /);
    await handle({
      kind: "accept",
      item: item({ availableFrom: "2999-01-01T00:00:00Z" }),
      listed: false,
    });
    expect(dialog.message).toHaveBeenCalledTimes(2);
    expect(accept.acceptAssignment).not.toHaveBeenCalled();
    // Not in the list: nothing to focus.
    expect(session.focus).toBeNull();
  });

  it("an unlisted class asks with the link's names before any request", async () => {
    dialog.ask.mockResolvedValue(false);
    await handle({ kind: "unlisted", link: acceptLink });
    expect(dialog.ask.mock.calls[0][0]).toBe(
      "Open the assignment lab3 of cs50/cs101? The app then reads the class's published assignment list.",
    );
    expect(api.fetchLinkedAssignment).not.toHaveBeenCalled();
    expect(accept.acceptAssignment).not.toHaveBeenCalled();
  });

  it("an unlisted class, once agreed, reads the manifest and accepts", async () => {
    dialog.ask.mockResolvedValue(true);
    const lab3 = item();
    api.fetchLinkedAssignment.mockResolvedValue({ kind: "accept", item: lab3, listed: false });
    await handle({ kind: "unlisted", link: acceptLink });
    expect(api.fetchLinkedAssignment).toHaveBeenCalledOnce();
    expect(accept.acceptAssignment).toHaveBeenCalledWith(lab3);
  });

  it("an unlisted class whose manifest lacks the assignment says so", async () => {
    dialog.ask.mockResolvedValue(true);
    api.fetchLinkedAssignment.mockResolvedValue({ kind: "missing", link: acceptLink });
    await handle({ kind: "unlisted", link: acceptLink });
    expect(dialog.message.mock.calls[0][0]).toBe(
      "The class cs101 has no open assignment named lab3. Check the link with your teacher.",
    );
  });

  it("a manifest that can't be read is worded", async () => {
    dialog.ask.mockResolvedValue(true);
    api.fetchLinkedAssignment.mockRejectedValue({ kind: "http", message: "404 Not Found" });
    await handle({ kind: "unlisted", link: acceptLink });
    expect(dialog.message).toHaveBeenCalledWith(
      "The link couldn't be opened: 404 Not Found",
      expect.objectContaining({ kind: "error" }),
    );
    expect(api.deepLinkDone).toHaveBeenCalledOnce();
  });

  it("a project not in the list says so", async () => {
    await handle({ kind: "missing", link: openLink });
    expect(dialog.message.mock.calls[0][0]).toContain("cs50/cs101-lab1-me isn't in your list");
  });

  it("is active while acting, and done even when acting fails", async () => {
    let during = false;
    accept.acceptAssignment.mockImplementation(async () => {
      during = linkActive();
      throw new Error("boom");
    });
    await expect(handle({ kind: "accept", item: item(), listed: true })).rejects.toThrow("boom");
    expect(during).toBe(true);
    expect(linkActive()).toBe(false);
    expect(api.deepLinkDone).toHaveBeenCalledOnce();
  });

  it("a call while one runs takes the next link after it, one at a time", async () => {
    let release!: () => void;
    accept.acceptAssignment.mockImplementationOnce(
      () => new Promise<void>((resolve) => (release = resolve)),
    );
    api.takeDeepLink
      .mockResolvedValueOnce({ kind: "accept", item: item(), listed: true })
      .mockResolvedValueOnce({ kind: "missing", link: openLink })
      .mockResolvedValue(null);
    const first = handleDeepLink();
    await vi.waitFor(() => expect(accept.acceptAssignment).toHaveBeenCalled());
    // Doesn't take a link itself: the running round does once it's done.
    await handleDeepLink();
    expect(api.takeDeepLink).toHaveBeenCalledOnce();
    release();
    await first;
    // One more round for the call that came meanwhile, then it stops.
    expect(api.takeDeepLink).toHaveBeenCalledTimes(2);
    expect(dialog.message.mock.calls[0][0]).toContain("isn't in your list");
    expect(api.deepLinkDone).toHaveBeenCalledTimes(2);
  });
});
