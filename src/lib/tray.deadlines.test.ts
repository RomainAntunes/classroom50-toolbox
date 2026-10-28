import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Assignment, Project, Settings, Snapshot, Unaccepted } from "#lib/api.js";
import { setLocale } from "#lib/i18n/index.js";
import { handleTrayAction, nextChange, trayMenu } from "#lib/tray.js";

const link = vi.hoisted(() => ({ focus: vi.fn() }));
vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => ({ page: { url: new URL("http://localhost/") } }));
vi.mock("#lib/api.js", () => ({ api: {} }));
vi.mock("#lib/deeplink.js", () => link);

// Local dates below are in Zurich time.
const zone = process.env.TZ;
// Made once the zone is set: a date built at load would be in the machine's own zone.
let NOW: Date;
beforeAll(() => {
  process.env.TZ = "Europe/Zurich";
  NOW = at(28, 15);
});
afterAll(() => {
  process.env.TZ = zone;
});
afterEach(() => setLocale("en"));

/** 28 October 2026 at 14:32 is `at(28, 14, 32)`, local time. */
const at = (day: number, hour: number, minute = 0) => new Date(2026, 9, day, hour, minute);
const iso = (day: number, hour: number, minute = 0) => at(day, hour, minute).toISOString();
// ICU puts a narrow no-break space before "PM".
const plain = (text: string) => text.replace(/\s/g, " ");
const HOUR = 3_600_000;

/** Lab `slug` of class cs101, accepted but not cloned: Rust says it needs work. */
function project(
  slug: string,
  due: string | null,
  over: Partial<Project> = {},
  assignment: Partial<Assignment> = {},
): Project {
  return {
    id: `acme/cs101-${slug}-alex`,
    owner: "acme",
    repo: `cs101-${slug}-alex`,
    config: { classroom: "cs101", assignment: slug, secret: null },
    path: null,
    local: null,
    remote: { archived: false, head: { oid: "abc" }, lastSubmission: null },
    classroom: null,
    assignment: { name: `Lab ${slug}`, due, closed: false, submissionMode: null, ...assignment },
    pending: null,
    needsWork: true,
    ...over,
  } as unknown as Project;
}

function unaccepted(slug: string, due: string | null, over: Partial<Assignment> = {}): Unaccepted {
  return {
    id: `acme/cs101/${slug}`,
    classroom: { org: "acme", slug: "cs101", name: "CS 101" },
    assignment: { name: `Lab ${slug}`, slug, due, closed: false, ...over },
    acceptUrl: "",
    group: null,
  } as unknown as Unaccepted;
}

function input(
  projects: Project[] = [],
  items: Unaccepted[] = [],
  settings: Partial<Settings> | null = null,
) {
  const snapshot = { projects, unaccepted: items, syncedAt: null, checkedAt: null } as Snapshot;
  return {
    snapshot,
    update: null,
    hidden: { ids: settings?.hidden ?? [], classes: settings?.hiddenClasses ?? [] },
    editor: false,
  };
}

const labels = (menu: ReturnType<typeof trayMenu>) =>
  menu.deadlines.map((item) => plain(item.label));

describe("deadlines", () => {
  it("names each deadline by the hour, nearest first", () => {
    const menu = trayMenu(
      input(
        [project("3", iso(29, 22, 30)), project("1", iso(28, 18))],
        [unaccepted("2", iso(29, 9))],
      ),
      NOW,
    );
    expect(menu.deadlines).toEqual([
      { id: "acme/cs101-1-alex", label: expect.any(String) },
      { id: "acme/cs101/2", label: expect.any(String) },
      { id: "acme/cs101-3-alex", label: expect.any(String) },
    ]);
    expect(labels(menu)).toEqual([
      "Lab 1 · Due today at 6:00 PM",
      "Lab 2 · Due tomorrow at 9:00 AM",
      "Lab 3 · Due tomorrow at 10:30 PM",
    ]);
  });

  it("keeps the 3 nearest", () => {
    const projects = [5, 1, 4, 2, 3].map((n) => project(`${n}`, iso(28 + n, 12)));
    expect(labels(trayMenu(input(projects), NOW)).map((l) => l.split(" · ")[0])).toEqual([
      "Lab 1",
      "Lab 2",
      "Lab 3",
    ]);
  });

  it("keeps a deadline for a day after it passes", () => {
    const menu = trayMenu(
      input([project("1", iso(28, 9)), project("2", iso(27, 16)), project("old", iso(27, 14))]),
      NOW,
    );
    expect(labels(menu)).toEqual([
      "Lab 2 · Was due yesterday at 4:00 PM",
      "Lab 1 · Was due today at 9:00 AM",
    ]);
  });

  it("leaves out work without a deadline, closed or archived", () => {
    const due = iso(29, 12);
    const projects = [
      project("none", null),
      project("closed", due, {}, { closed: true }),
      project("archived", due, { remote: { archived: true } } as Partial<Project>),
      project("unknown", due, { assignment: null }),
    ];
    const items = [unaccepted("none", null), unaccepted("closed", due, { closed: true })];
    expect(trayMenu(input(projects, items), NOW).deadlines).toEqual([]);
  });

  it("leaves out work that is done, as Rust's needsWork says (reminders go by it too)", () => {
    const due = iso(29, 12);
    const done = project("done", due, { path: "~/c/done", needsWork: false });
    const changed = project("changed", due, { path: "~/c/changed" });
    const menu = trayMenu(input([done, changed]), NOW);
    expect(labels(menu).map((l) => l.split(" · ")[0])).toEqual(["Lab changed"]);
  });

  it("leaves out hidden assignments and classes", () => {
    const due = iso(29, 12);
    const other = project("other", due, {
      id: "acme/math-other-alex",
      owner: "Acme",
      config: { classroom: "Math", assignment: "other", secret: null },
    });
    const all = input([project("1", due), other], [unaccepted("2", due)]);
    expect(trayMenu(all, NOW).deadlines).toHaveLength(3);
    const hidden = input([project("1", due), other], [unaccepted("2", due)], {
      hidden: ["acme/cs101-1-alex"],
      hiddenClasses: ["acme/math"],
    });
    expect(trayMenu(hidden, NOW).deadlines.map((d) => d.id)).toEqual(["acme/cs101/2"]);
    const items = input([], [unaccepted("2", due)], { hiddenClasses: ["acme/cs101"] });
    expect(trayMenu(items, NOW).deadlines).toEqual([]);
  });

  it("speaks the UI's language", () => {
    setLocale("fr");
    const menu = trayMenu(input([project("1", iso(29, 22, 30))]), NOW);
    expect(labels(menu)).toEqual(["Lab 1 · À rendre demain à 22:30"]);
  });
});

describe("nextChange with deadlines", () => {
  it("wakes when a deadline passes", () => {
    expect(nextChange(input([project("1", iso(28, 18))]), NOW)).toBe(3 * HOUR);
  });

  it("wakes a day after one, when it drops", () => {
    expect(nextChange(input([project("1", iso(27, 17))]), NOW)).toBe(2 * HOUR);
  });

  it("watches deadlines past the 3 shown", () => {
    // Three were due last night and fill the menu; the fourth is due at 5 PM.
    const projects = [
      project("1", iso(27, 20)),
      project("2", iso(27, 21)),
      project("3", iso(27, 22)),
      project("4", iso(28, 17)),
    ];
    expect(trayMenu(input(projects), NOW).deadlines).toHaveLength(3);
    expect(nextChange(input(projects), NOW)).toBe(2 * HOUR);
  });

  it("is when Rust asks for a new menu", () => {
    expect(trayMenu(input([project("1", iso(28, 18))]), NOW).validUntil).toBe(iso(28, 18));
    expect(trayMenu(input(), NOW).validUntil).toBeNull();
  });

  it("ignores what is past or hidden, and keeps midnight", () => {
    const projects = [project("old", iso(26, 12)), project("1", iso(28, 16))];
    const hidden = input(projects, [], { hidden: ["acme/cs101-1-alex"] });
    expect(nextChange(hidden, NOW)).toBe(9 * HOUR);
    expect(nextChange(input([project("1", iso(30, 12))]), NOW)).toBe(9 * HOUR);
  });
});

describe("a deadline from the menu", () => {
  it("shows its row, a project or an assignment not accepted yet", async () => {
    await handleTrayAction({ kind: "focus", id: "acme/cs101-1-alex" });
    expect(link.focus).toHaveBeenLastCalledWith("acme/cs101-1-alex");
    await handleTrayAction({ kind: "focus", id: "acme/cs101/2" });
    expect(link.focus).toHaveBeenLastCalledWith("acme/cs101/2");
  });
});
