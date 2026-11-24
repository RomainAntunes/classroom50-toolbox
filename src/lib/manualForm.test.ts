import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { draftFor, formProblem, parseRepo, toManualDraft } from "#lib/manualForm.js";
import type { ManualInit } from "#lib/manualEditor.svelte.js";

// Local dates below are in Zurich time.
const zone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Zurich";
});
afterAll(() => {
  process.env.TZ = zone;
});

const init = (over: Partial<ManualInit> = {}): ManualInit => ({
  id: null,
  name: "",
  class: "",
  due: null,
  notes: "",
  done: false,
  path: null,
  repo: "",
  ...over,
});
const now = new Date("2026-10-28T14:00:00Z");

describe("draftFor", () => {
  it("dates a project without a due date a week ahead, unticked", () => {
    const draft = draftFor(init(), now);
    expect(draft).toMatchObject({ hasDue: false, date: "2026-11-04", time: "23:59" });
  });

  it("reads a due date in local time", () => {
    const draft = draftFor(init({ due: "2026-10-29T17:00:00Z" }), now);
    expect(draft).toMatchObject({ hasDue: true, date: "2026-10-29", time: "18:00" });
  });
});

describe("formProblem", () => {
  const ok = () => draftFor(init({ name: "Essay" }), now);

  it("needs a name", () => {
    expect(formProblem({ ...ok(), name: "  " })).toBe("name");
    expect(formProblem(ok())).toBeNull();
  });

  it("checks the date only when there is one", () => {
    expect(formProblem({ ...ok(), date: "" })).toBeNull();
    expect(formProblem({ ...ok(), hasDue: true, date: "" })).toBe("date");
    expect(formProblem({ ...ok(), hasDue: true, date: "2026-02-31" })).toBe("date");
  });

  it("checks a repo when one is typed", () => {
    expect(formProblem({ ...ok(), repo: "nope" })).toBe("repo");
    expect(formProblem({ ...ok(), repo: "octo/essay" })).toBeNull();
  });
});

describe("parseRepo", () => {
  it("reads typed and pasted repos as Rust does", () => {
    for (const text of [
      "octo/essay",
      " octo/essay.git ",
      "github.com/octo/essay",
      "www.github.com/octo/essay/",
      "https://github.com/octo/essay",
      "git@github.com:octo/essay.git",
    ]) {
      expect(parseRepo(text), text).toBe("octo/essay");
    }
    for (const text of [
      "octo",
      "octo/",
      "/essay",
      "-octo/essay",
      "octo/es say",
      "https://gitlab.com/octo/essay",
    ]) {
      expect(parseRepo(text), text).toBeNull();
    }
  });
});

describe("toManualDraft", () => {
  it("sends blank fields as none and the due date as UTC", () => {
    const draft = {
      ...draftFor(init({ name: " Essay ", class: "  " }), now),
      hasDue: true,
      date: "2026-10-30",
      time: "12:00",
      repo: "https://github.com/octo/essay",
    };
    expect(toManualDraft(draft, "manual:1")).toEqual({
      id: "manual:1",
      name: "Essay",
      class: null,
      due: "2026-10-30T11:00:00.000Z",
      notes: null,
      done: false,
      path: null,
      repo: "octo/essay",
    });
    expect(toManualDraft({ ...draft, hasDue: false }, null).due).toBeNull();
  });
});
