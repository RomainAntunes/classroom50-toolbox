import { describe, expect, it } from "vitest";
import type { Assignment } from "#lib/api.js";
import { gradingDetail, gradingMode, passing, passLabel, showsGrading } from "#lib/grading.js";

const assignment = (over: Partial<Assignment>): Assignment =>
  ({ slug: "lab-1", name: "Lab 1", mode: "individual", feedbackPr: false, ...over }) as Assignment;

describe("grading", () => {
  it("reads an absent grading as autograded", () => {
    expect(gradingMode(null)).toBe("auto");
    expect(gradingMode(assignment({ grading: null }))).toBe("auto");
    expect(gradingMode(assignment({ grading: { mode: "off", maxPoints: null } }))).toBe("off");
    expect(gradingDetail(assignment({ grading: null }))).toBeNull();
    expect(gradingDetail(assignment({ grading: { mode: "auto", maxPoints: null } }))).toBeNull();
  });

  it("says an ungraded assignment isn't graded", () => {
    expect(gradingDetail(assignment({ grading: { mode: "off", maxPoints: null } }))).toBe(
      "Not graded",
    );
  });

  it("names the points of a manual grading and where its feedback comes", () => {
    const manual = { mode: "manual", maxPoints: 50 } as const;
    const [by, where] = gradingDetail(assignment({ grading: manual }))!.split("\n");
    expect(by).toBe("By your teaching staff, out of 50 points");
    expect(where).toContain("assignment's page on Classroom 50");
    const pr = gradingDetail(assignment({ grading: manual, feedbackPr: true }))!;
    expect(pr).toContain("Feedback pull request");
    expect(gradingDetail(assignment({ grading: { mode: "manual", maxPoints: 1 } }))).toContain(
      "out of 1 point\n",
    );
    expect(gradingDetail(assignment({ grading: { mode: "manual", maxPoints: null } }))).toMatch(
      /^By your teaching staff\n/,
    );
  });

  it("judges a grade against the passing bar only when there is one", () => {
    expect(passing({ score: 6, maxScore: 10 }, 60)).toBe(true);
    expect(passing({ score: 5, maxScore: 10 }, 60)).toBe(false);
    // Absent is no bar at all, not 0%.
    expect(passing({ score: 0, maxScore: 10 }, null)).toBeNull();
    expect(passing({ score: 0, maxScore: 10 }, 0)).toBe(true);
    expect(passing(null, 60)).toBeNull();
    expect(passing({ score: 0, maxScore: 0 }, 60)).toBeNull();
    expect(passLabel({ score: 7, maxScore: 12 }, 50)).toBe("Passing");
    expect(passLabel({ score: 5, maxScore: 12 }, 50)).toBe("Below the passing bar (50%)");
    expect(passLabel({ score: 5, maxScore: 12 }, undefined)).toBeNull();
  });

  it("hides how it is graded only while nothing changed and nothing is graded", () => {
    expect(showsGrading(true, false, false)).toBe(true);
    expect(showsGrading(false, false, false)).toBe(false);
    // The teacher changed its grading since, or a grade came in: it shows again.
    expect(showsGrading(false, true, false)).toBe(true);
    expect(showsGrading(false, false, true)).toBe(true);
  });
});
