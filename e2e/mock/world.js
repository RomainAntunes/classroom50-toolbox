// What the mock GitHub knows: one signed-in student in two classes, built relative to `now`
// so due dates stay ahead of (or behind) the test run. Field names follow upstream's schemas
// (`assignments-v1`, `result-v1`, `classroom-team-v1`) and GitHub's GraphQL types.

export const LOGIN = "ada-e2e";

const HOUR = 60 * 60 * 1000;
const DAY = 24 * HOUR;

/** `2026-10-14T12-00-05Z` for a submit tag (upstream's `submit/<UTC-timestamp>-<short-sha>`). */
function stamp(date) {
  return date
    .toISOString()
    .replace(/\.\d{3}Z$/, "Z")
    .replaceAll(":", "-");
}

function teamRecord(name, term) {
  return JSON.stringify({ schema: "classroom50/team/v1", name, term, active: true });
}

/**
 * The fixture world.
 * @param {{ now?: Date, heads?: { hello?: string, helloSubmitted?: string } }} [options]
 *   `heads` are the fixture clone's commits: GitHub's head of the hello repo is the clone's
 *   HEAD (so the clone is up to date), and its last submission an older commit.
 */
export function buildWorld({ now = new Date(), heads = {} } = {}) {
  const at = (offset) => new Date(now.getTime() + offset);
  const iso = (offset) =>
    at(offset)
      .toISOString()
      .replace(/\.\d{3}Z$/, "Z");
  const hello = heads.hello ?? "1".repeat(40);
  const helloSubmitted = heads.helloSubmitted ?? "2".repeat(40);
  const submitTag = `submit/${stamp(at(-DAY))}-${helloSubmitted.slice(0, 7)}`;

  return {
    login: LOGIN,
    orgs: [
      {
        login: "c50-e2e-school",
        teams: [{ slug: "classroom50-algo", description: teamRecord("Algorithms", "2026-S1") }],
      },
      {
        login: "c50-e2e-lab",
        teams: [{ slug: "classroom50-web", description: teamRecord("Web Programming", "2026-S1") }],
      },
    ],
    manifests: {
      "c50-e2e-school/algo": {
        schema: "classroom50/assignments/v1",
        assignments: [
          {
            slug: "hello",
            name: "Hello, World",
            mode: "individual",
            autograder: "default",
            description: "Print a greeting, then a farewell.",
            due: iso(3 * DAY),
            submission_mode: "tag",
          },
          {
            slug: "sorting",
            name: "Sorting",
            mode: "individual",
            autograder: "default",
            due: iso(-2 * DAY),
          },
          {
            slug: "graphs",
            name: "Graphs",
            mode: "individual",
            autograder: "default",
            due: iso(10 * DAY),
          },
        ],
      },
      "c50-e2e-lab/web": {
        schema: "classroom50/assignments/v1",
        assignments: [
          {
            slug: "portfolio",
            name: "Portfolio",
            mode: "individual",
            autograder: "empty_repo",
            due: iso(7 * DAY),
            // Renamed with `gh teacher assignment rename`; repos named after the old slug
            // still count.
            renamed_from: "website",
          },
          {
            slug: "blog",
            name: "Blog",
            mode: "individual",
            autograder: "no_autograder",
            due: iso(14 * DAY),
            template: { owner: "c50-e2e-lab", repo: "blog-starter", branch: "main" },
          },
        ],
      },
    },
    repos: [
      {
        owner: "c50-e2e-school",
        name: "algo-hello-ada-e2e",
        pushedAt: iso(-HOUR),
        head: { oid: hello, committedDate: iso(-HOUR) },
        config: "classroom: algo\nassignment: hello\n",
        submissions: [{ tag: submitTag, commit: helloSubmitted }],
        releases: [
          {
            tagName: submitTag,
            publishedAt: iso(-DAY + HOUR),
            result: {
              schema: "classroom50/result/v1",
              classroom: "algo",
              assignment: "hello",
              assignment_type: "individual",
              owner: LOGIN,
              submission: submitTag,
              commit: `https://github.com/c50-e2e-school/algo-hello-ada-e2e/commit/${helloSubmitted}`,
              release: `https://github.com/c50-e2e-school/algo-hello-ada-e2e/releases/tag/${submitTag}`,
              review: "https://github.com/c50-e2e-school/algo-hello-ada-e2e/compare/a...b",
              datetime: iso(-DAY),
              score: 7,
              "max-score": 12,
              tests: [
                { "test-name": "compiles", passed: true, score: 3, "max-score": 3 },
                { "test-name": "prints hello", passed: true, score: 4, "max-score": 4 },
                { "test-name": "prints goodbye", passed: false, score: 0, "max-score": 3 },
                { "test-name": "asks for a name", passed: false, score: 0, "max-score": 2 },
              ],
            },
            body: [
              "### classroom50 autograde: 7/12",
              "",
              "| Test | Result | Score |",
              "|---|---|---|",
              "| compiles | PASS | 3 / 3 |",
              "| prints hello | PASS | 4 / 4 |",
              "| prints goodbye | FAIL | 0 / 3 |",
              "| asks for a name | FAIL | 0 / 2 |",
              "",
              "<details><summary>Failure details</summary>",
              "",
              "**prints goodbye**",
              "",
              "```",
              "exit 0; comparison=exact",
              "--- expected",
              "+++ actual stdout",
              "@@ -1 +1 @@",
              "-goodbye, world",
              "+hello, world",
              "```",
              "",
              // A test the runner stopped (cli 1.58.0): what it was fed and printed first.
              "**asks for a name**",
              "",
              "```",
              "timed out after 10s",
              "--- input ---",
              "Ada",
              "",
              "--- actual stdout ---",
              "What's your name? ",
              "```",
              "",
              "</details>",
              "",
              "Status: classroom50 autograde: 7/12 (2/4 tests passed)",
              "",
            ].join("\n"),
          },
        ],
      },
      {
        owner: "c50-e2e-school",
        name: "algo-sorting-ada-e2e",
        pushedAt: iso(-DAY),
        // Pushed a day after the deadline: late, never refused.
        head: { oid: "3".repeat(40), committedDate: iso(-DAY) },
        config: "classroom: algo\nassignment: sorting\n",
        submissions: [],
        releases: [],
      },
      {
        // An `empty_repo` assignment: no marker, known by its name.
        owner: "c50-e2e-lab",
        name: "web-portfolio-ada-e2e",
        pushedAt: iso(-2 * DAY),
        head: { oid: "4".repeat(40), committedDate: iso(-2 * DAY) },
        config: null,
        submissions: [],
        releases: [],
      },
      {
        // Renamed by hand and its marker deleted: known by the template it came from.
        owner: "c50-e2e-lab",
        name: "ada-writes",
        pushedAt: iso(-3 * DAY),
        head: { oid: "6".repeat(40), committedDate: iso(-3 * DAY) },
        config: null,
        template: "c50-e2e-lab/blog-starter",
        submissions: [],
        releases: [],
      },
      {
        // A teacher's copy of the Blog starter the student can only read: never their Blog.
        owner: "c50-e2e-lab",
        name: "blog-solution",
        pushedAt: iso(-4 * DAY),
        head: { oid: "7".repeat(40), committedDate: iso(-4 * DAY) },
        config: null,
        template: "c50-e2e-lab/blog-starter",
        permission: "READ",
        submissions: [],
        releases: [],
      },
      {
        // Not a Classroom 50 repo: never listed.
        owner: LOGIN,
        name: "dotfiles",
        pushedAt: iso(-5 * DAY),
        head: { oid: "5".repeat(40), committedDate: iso(-5 * DAY) },
        config: null,
        submissions: [],
        releases: [],
      },
    ],
  };
}
