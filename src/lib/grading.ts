// Whether and how an assignment is graded, and whether a grade reaches its passing bar, as
// the teacher published them in the class's `assignments.json` (`grading`,
// `pass_threshold`). Read-only: the marks of a manual grading stay private upstream.
import type { Assignment } from "#lib/api.js";
import { t } from "#lib/i18n/index.js";

export type GradingMode = "off" | "auto" | "manual";

/** How the assignment is graded; absent means autograded. */
export function gradingMode(assignment: Assignment | null | undefined): GradingMode {
  return assignment?.grading?.mode ?? "auto";
}

/** Whether `grade` reaches the passing bar, or null with no bar (absent isn't 0%) or no
 *  grade to judge. */
export function passing(
  grade: { score: number; maxScore: number } | null,
  threshold: number | null | undefined,
): boolean | null {
  if (!grade || threshold === null || threshold === undefined || grade.maxScore <= 0) return null;
  return grade.score * 100 >= threshold * grade.maxScore;
}

/** "Passing" or "Below the passing bar (60%)", null when there's nothing to say. */
export function passLabel(
  grade: { score: number; maxScore: number } | null,
  threshold: number | null | undefined,
): string | null {
  const pass = passing(grade, threshold);
  if (pass === null) return null;
  return pass ? t("projects.passing") : t("projects.belowPassing", { percent: threshold! });
}

/** Whether a row says how its assignment is graded: unless the student hid it
 *  (`showGradingInfo`), and then still once the teacher changed its grading since (`revealed`,
 *  from Rust) or it has a grade. */
export function showsGrading(shown: boolean, revealed: boolean, hasGrade: boolean): boolean {
  return shown || revealed || hasGrade;
}

/** The details' "Grading" line for an assignment that isn't autograded: not graded, or by
 *  the teaching staff out of so many points, with where their feedback comes since the
 *  mark itself isn't shown to students. Null for autograded ones. */
export function gradingDetail(assignment: Assignment | null | undefined): string | null {
  if (!assignment) return null;
  switch (gradingMode(assignment)) {
    case "off":
      return t("projects.notGraded");
    case "manual": {
      const points = assignment.grading?.maxPoints ?? null;
      const by =
        points === null
          ? t("details.gradingManualNoPoints")
          : t("details.gradingManual", { count: points });
      const where = t(
        assignment.feedbackPr ? "details.gradingManualFeedback" : "details.gradingManualPage",
      );
      return `${by}\n${where}`;
    }
    default:
      return null;
  }
}
