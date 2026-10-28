// A list with every kind of row, for layout checks and screenshots: late, graded, grading,
// uncommitted, local changes, commit then pull, behind, feedback, closed, a group, not
// cloned, unaccepted and upcoming.
import {
  classroom,
  daysFromNow,
  grade,
  gradeTest,
  hoursFromNow,
  project,
  scenario,
  settings,
  snapshot,
  tools,
  unaccepted,
  type Scenario,
} from "./fixtures";
import type { Settings } from "../../src/lib/api";

const cs101 = classroom();
const art = classroom({ org: "art-school", slug: "art200", name: "Drawing and Illustration" });

export function richScenario(over: Partial<Settings> = {}): Scenario {
  return scenario({
    settings: settings({ commitActions: "push", ...over }),
    tools: tools().map((tool) =>
      tool.tool === "gh" ? { ...tool, version: "2.40.0", outdated: true } : tool,
    ),
    snapshot: snapshot({
      projects: [
        project({
          slug: "lab1",
          name: "Lab 1: Hello, World",
          assignment: { due: hoursFromNow(-30) },
          lateSeconds: 26 * 3600,
          remote: {
            grade: grade(8, 10, {
              tests: [
                gradeTest("compiles", { passed: true, score: 3, maxScore: 3 }),
                gradeTest("prints hello", { passed: true, score: 5, maxScore: 5 }),
                // A test the autograder stopped at its time limit, with what it printed.
                gradeTest("rejects a negative height", {
                  maxScore: 2,
                  output: "--- input ---\n-3\n\n--- actual stdout ---\nHeight: Height: ",
                  timedOutAfter: 10,
                }),
              ],
              releaseNotes: { notes: null, notesClipped: false },
            }),
            feedback: { url: "https://github.com/pr/1", comments: 3, latestAt: hoursFromNow(-1) },
          },
        }),
        project({
          slug: "lab2",
          name: "Lab 2: Loops and Conditions",
          assignment: { due: hoursFromNow(5) },
          pending: "uncommitted",
          local: { changedFiles: 3, changedPaths: ["mario.c", "cash.c", "README.md"] },
        }),
        // Every push is graded and GitHub has the clone's HEAD: the edits are only local.
        project({
          slug: "lab6",
          name: "Lab 6: Recursion",
          assignment: { submissionMode: "every-push", due: daysFromNow(2) },
          pending: "uncommitted",
          local: { changedFiles: 1, changedPaths: ["notes.md"] },
        }),
        // A teammate pushed over changes to tracked files, which the app's Pull refuses.
        project({
          slug: "lab7",
          name: "Lab 7: Pointers",
          assignment: { submissionMode: "every-push", due: daysFromNow(3) },
          pending: "conflict",
          local: {
            remoteAhead: true,
            changedFiles: 1,
            trackedChanges: 1,
            changedPaths: ["swap.c"],
          },
        }),
        project({
          slug: "lab3",
          name: "Lab 3: Arrays",
          assignment: { submissionMode: "every-push", due: daysFromNow(4) },
          local: { behind: 2 },
          remote: { grading: { state: "running", url: null } },
        }),
        project({
          slug: "proj",
          name: "Final Project with a Rather Long Name for Wrapping",
          assignment: { mode: "team", teamFormation: "student", due: daysFromNow(20) },
          local: null,
          group: {
            org: cs101.org,
            slug: "classroom50-group-abc-4",
            number: 4,
            name: "The Sharks",
            members: [
              { login: "student", name: "Stu Dent", email: null },
              { login: "ada", name: "Ada Lovelace", email: "ada@example.com" },
            ],
            founders: ["student"],
            url: "https://github.com/orgs/cs50-school/teams/x",
            classroom: null,
            assignment: null,
          },
        }),
        project({
          slug: "sketch",
          name: "Sketchbook",
          classroom: art,
          assignment: { closed: true, due: daysFromNow(-10) },
          remote: { grade: grade(10, 10) },
        }),
      ],
      unaccepted: [
        unaccepted({ slug: "lab4", name: "Lab 4: Memory", classroom: cs101, due: daysFromNow(9) }),
        unaccepted({
          slug: "lab5",
          name: "Lab 5: Data Structures",
          classroom: cs101,
          availableFrom: daysFromNow(6),
          due: daysFromNow(13),
        }),
      ],
    }),
  });
}
