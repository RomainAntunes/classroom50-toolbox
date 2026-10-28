import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Snapshot, TrayMenu } from "#lib/api.js";
import { setLocale } from "#lib/i18n/index.js";
import { handleTrayAction, nextChange, sendTrayMenu, trayMenu } from "#lib/tray.js";

const nav = vi.hoisted(() => ({ goto: vi.fn() }));
const state = vi.hoisted(() => ({ page: { url: new URL("http://localhost/") } }));
const api = vi.hoisted(() => ({ setTrayMenu: vi.fn() }));
vi.mock("$app/navigation", () => nav);
vi.mock("$app/state", () => state);
vi.mock("#lib/api.js", () => ({ api }));

// Local dates below are in Zurich time.
const zone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Zurich";
});
afterAll(() => {
  process.env.TZ = zone;
});
afterEach(() => setLocale("en"));

/** 28 October 2026 at 14:32 is `at(28, 14, 32)`, local time. */
const at = (day: number, hour: number, minute = 0) => new Date(2026, 9, day, hour, minute);
// ICU puts a narrow no-break space before "PM".
const plain = (text: string | null) => text?.replace(/\s/g, " ") ?? null;

function snapshot(over: Partial<Snapshot> = {}): Snapshot {
  return {
    projects: [],
    unaccepted: [],
    syncedAt: at(28, 14, 32).toISOString(),
    checkedAt: null,
    login: "alex",
    issue: null,
    ...over,
  };
}

const input = (snap: Snapshot | null = snapshot()) => ({
  snapshot: snap,
  update: null,
  settings: null,
  editors: [],
});

describe("trayMenu", () => {
  it("words every item", () => {
    const menu = trayMenu(input(), at(28, 15));
    expect({ ...menu, synced: plain(menu.synced) }).toEqual({
      open: "Open Classroom 50 Toolbox",
      quit: "Quit",
      refresh: "Refresh",
      settings: "Settings…",
      synced: "Synced today at 2:32 PM",
      deadlines: [],
      recentTitle: "Open Recent",
      recent: [],
      update: null,
    } satisfies TrayMenu);
  });

  it("says when GitHub was read last, a status check included", () => {
    const menu = trayMenu(
      input(snapshot({ checkedAt: at(28, 15, 10).toISOString() })),
      at(28, 15, 20),
    );
    expect(plain(menu.synced)).toBe("Synced today at 3:10 PM");
  });

  it("names the hour, never a countdown, so the line holds until midnight", () => {
    // 10 minutes after the sync the list's footer says "10 minutes ago".
    expect(plain(trayMenu(input(), at(28, 14, 42)).synced)).toBe("Synced today at 2:32 PM");
    expect(plain(trayMenu(input(), at(29, 9)).synced)).toBe("Synced yesterday at 2:32 PM");
  });

  it("has no synced line before the first sync", () => {
    expect(trayMenu(input(null), at(28, 15)).synced).toBeNull();
    const never = snapshot({ syncedAt: null, checkedAt: null });
    expect(trayMenu(input(never), at(28, 15)).synced).toBeNull();
  });

  it("speaks the UI's language", () => {
    setLocale("fr");
    const menu = trayMenu(input(), at(28, 15));
    expect(menu.refresh).toBe("Actualiser");
    expect(menu.settings).toBe("Réglages…");
    expect(menu.recentTitle).toBe("Ouvrir un élément récent");
    expect(menu.synced).toBe("Synchronisé aujourd’hui à 14:32");
  });
});

describe("nextChange", () => {
  it("wakes at the next local midnight", () => {
    expect(nextChange(input(), at(28, 15))).toBe(9 * 3_600_000);
    expect(nextChange(input(), at(28, 23, 59))).toBe(60_000);
  });

  it("counts the 25-hour day when summer time ends", () => {
    // 25 October 2026 holds an extra hour.
    expect(nextChange(input(), at(25, 0))).toBe(25 * 3_600_000);
  });
});

describe("sendTrayMenu", () => {
  beforeEach(() => api.setTrayMenu.mockReset().mockResolvedValue(null));

  it("sends a menu only when it changed", () => {
    const first = trayMenu(input(), at(28, 15));
    sendTrayMenu(first);
    sendTrayMenu({ ...first });
    expect(api.setTrayMenu).toHaveBeenCalledTimes(1);
    sendTrayMenu({ ...first, refresh: "Actualiser" });
    expect(api.setTrayMenu).toHaveBeenCalledTimes(2);
    expect(api.setTrayMenu).toHaveBeenLastCalledWith({ ...first, refresh: "Actualiser" });
  });

  it("sends it again after a failure", async () => {
    api.setTrayMenu.mockRejectedValueOnce(new Error("no tray"));
    const menu = { ...trayMenu(input(), at(28, 15)), quit: "Beenden" };
    sendTrayMenu(menu);
    await vi.waitFor(() => expect(api.setTrayMenu).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    sendTrayMenu(menu);
    expect(api.setTrayMenu).toHaveBeenCalledTimes(2);
  });
});

describe("handleTrayAction", () => {
  beforeEach(() => nav.goto.mockReset());

  it("opens Settings", async () => {
    state.page.url = new URL("http://localhost/");
    await handleTrayAction({ kind: "settings" });
    expect(nav.goto).toHaveBeenCalledWith("/settings");
  });

  it("leaves the first-run guide where it is", async () => {
    state.page.url = new URL("http://localhost/welcome");
    await handleTrayAction({ kind: "settings" });
    expect(nav.goto).not.toHaveBeenCalled();
  });
});
