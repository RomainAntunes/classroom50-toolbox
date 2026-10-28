import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CommandError } from "#lib/api.js";

const dialog = vi.hoisted(() => ({ message: vi.fn() }));
const api = vi.hoisted(() => ({ installUpdate: vi.fn() }));
const withDialog = vi.hoisted(() => vi.fn(<T>(run: () => Promise<T>) => run()));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("#lib/api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/api.js")>()),
  api,
  withDialog,
}));

const { installUpdate } = await import("#lib/update.js");

const failure = (kind: string, message: string) =>
  ({ kind, message, params: {} }) as unknown as CommandError;

beforeEach(() => {
  vi.clearAllMocks();
});

describe("installUpdate", () => {
  it("installs and shows nothing when Rust takes over", async () => {
    api.installUpdate.mockResolvedValue(undefined);
    await installUpdate();
    expect(api.installUpdate).toHaveBeenCalledOnce();
    expect(dialog.message).not.toHaveBeenCalled();
  });

  it("shows a failure in an error dialog that keeps the popover open", async () => {
    api.installUpdate.mockRejectedValue(failure("updater", "signature mismatch"));
    await installUpdate();
    expect(withDialog).toHaveBeenCalledOnce();
    expect(dialog.message).toHaveBeenCalledExactlyOnceWith("signature mismatch", {
      kind: "error",
    });
  });

  it("says nothing when an install already runs", async () => {
    api.installUpdate.mockRejectedValue(failure("busy", "already running"));
    await installUpdate();
    expect(dialog.message).not.toHaveBeenCalled();
  });
});
