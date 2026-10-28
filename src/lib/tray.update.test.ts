import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateState, UpdateStatus } from "#lib/api.js";
import { setLocale } from "#lib/i18n/index.js";
import { handleTrayAction, readyVersion, trayMenu } from "#lib/tray.js";

const installUpdate = vi.hoisted(() => vi.fn());
vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => ({ page: { url: new URL("http://localhost/") } }));
vi.mock("#lib/api.js", () => ({ api: { setTrayMenu: vi.fn() } }));
vi.mock("#lib/update.js", () => ({ installUpdate }));

afterEach(() => setLocale("en"));

const ready: UpdateState = { phase: "ready", version: "0.14.0", notes: null };

function status(state: UpdateState, over: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    state,
    deferred: false,
    installing: false,
    simulated: false,
    lastCheck: null,
    bundle: "app",
    canUpdate: true,
    installOnQuit: true,
    asksPassword: false,
    hasBeta: true,
    ...over,
  };
}

/** The menu as the layout computes it: from the ready version alone, so a download's
 *  progress never recomputes it. */
const menu = (update: UpdateStatus | null) =>
  trayMenu(
    {
      snapshot: null,
      update: readyVersion(update),
      hidden: { ids: [], classes: [] },
      editor: false,
    },
    new Date(),
  );

describe("the update item", () => {
  it("offers the restart once the update is downloaded", () => {
    expect(menu(status(ready)).update).toBe("Restart to Update to 0.14.0");
  });

  it("stays after Later, which only drops the banner's question", () => {
    expect(menu(status(ready, { deferred: true })).update).toBe("Restart to Update to 0.14.0");
  });

  it("goes while it installs", () => {
    expect(menu(status(ready, { installing: true })).update).toBeNull();
  });

  it("is absent with nothing downloaded", () => {
    expect(menu(null).update).toBeNull();
    expect(menu(status({ phase: "idle" })).update).toBeNull();
    expect(menu(status({ phase: "checking" })).update).toBeNull();
    const downloading: UpdateState = {
      phase: "downloading",
      version: "0.14.0",
      downloaded: 10,
      total: 100,
    };
    expect(menu(status(downloading)).update).toBeNull();
    // Every progress step reads the same: the layout's `$derived` doesn't move.
    expect(readyVersion(status({ ...downloading, downloaded: 50 }))).toBeNull();
  });

  it("follows the UI's language", () => {
    setLocale("fr");
    expect(menu(status(ready)).update).toBe("Redémarrer pour installer la version 0.14.0");
  });
});

describe("handleTrayAction", () => {
  beforeEach(() => installUpdate.mockReset());

  it("installs as the banner's Restart now does", async () => {
    await handleTrayAction({ kind: "installUpdate" });
    expect(installUpdate).toHaveBeenCalledOnce();
  });
});
