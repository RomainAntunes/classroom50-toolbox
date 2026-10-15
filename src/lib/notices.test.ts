import { describe, expect, it, vi } from "vitest";

const sent: { title: string; body: string }[] = [];
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: async () => true,
  requestPermission: async () => "granted",
  sendNotification: (notice: { title: string; body: string }) => sent.push(notice),
}));

const { sampleNotices, showNotice } = await import("$lib/notices");
const en: Record<string, string> = (await import("$lib/i18n/en.json")).default;

describe("notices", () => {
  it("has a dev label, a title and a body for every kind", () => {
    for (const { kind } of sampleNotices()) {
      for (const key of [
        `settings.dev.kind.${kind}`,
        `alerts.${kind}.title`,
        `alerts.${kind}.body`,
      ]) {
        expect(en[key], key).toBeTruthy();
      }
    }
  });

  it("leaves no placeholder unfilled", async () => {
    sent.length = 0;
    for (const notice of sampleNotices()) await showNotice(notice);
    expect(sent).toHaveLength(sampleNotices().length);
    for (const { title, body } of sent) {
      expect(`${title} ${body}`).not.toMatch(/\{\w+\}/);
    }
  });

  it("names everyone who joined a group", async () => {
    sent.length = 0;
    await showNotice({ kind: "groupJoined", assignment: "Lab 1", members: ["Ada", "Bob"] });
    expect(sent[0].title).toBe("Ada and Bob joined your group");
  });
});
