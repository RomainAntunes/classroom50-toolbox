import { describe, expect, it, vi } from "vitest";
import type { SshStatus } from "#lib/api.js";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), message: vi.fn() }));

const { statusText, hostKeyProblem, protocolName } = await import("#lib/ssh.js");

describe("ssh", () => {
  it("words every state in one sentence, with its data", () => {
    const cases: [SshStatus, string][] = [
      [{ state: "ready", login: "alex" }, "Ready as alex"],
      [{ state: "ready", login: null }, "Ready"],
      [
        { state: "otherAccount", login: "old-alex", active: "alex" },
        "This computer's SSH key signs in to GitHub as old-alex, not alex, so it only reaches the repositories old-alex can.",
      ],
      [
        { state: "otherAccount", login: "acme/site", active: "alex" },
        "This computer's SSH key is a deploy key of acme/site, so it reaches no other repository.",
      ],
      [
        { state: "noKey", localKey: null },
        "No SSH key of this computer is on your GitHub account.",
      ],
      [
        { state: "keyNotOffered", localKey: "~/.ssh/id_rsa.pub" },
        "Your key ~/.ssh/id_rsa.pub is on GitHub, but ssh didn't use it. If it has a passphrase, load it with ssh-add in a terminal.",
      ],
      [{ state: "hostKeyUnknown" }, "This computer doesn't know GitHub's SSH host key yet."],
      [
        { state: "unreachable", message: "timed out" },
        "Can't reach GitHub over SSH: this network may block it. HTTPS still works.",
      ],
      [{ state: "notInstalled" }, "ssh isn't installed on this computer."],
      [{ state: "failed", message: "boom" }, "Couldn't check SSH: boom"],
    ];
    for (const [status, text] of cases) expect(statusText(status)).toBe(text);
    expect(statusText({ state: "hostKeyChanged" })).toMatch(/replaced in 2023\.$/);
  });

  it("tells host key problems apart", () => {
    expect(hostKeyProblem({ state: "hostKeyUnknown" })).toBe(true);
    expect(hostKeyProblem({ state: "hostKeyChanged" })).toBe(true);
    expect(hostKeyProblem({ state: "noKey", localKey: null })).toBe(false);
  });

  it("names the protocols", () => {
    expect([protocolName("https"), protocolName("ssh")]).toEqual(["HTTPS", "SSH"]);
  });
});
