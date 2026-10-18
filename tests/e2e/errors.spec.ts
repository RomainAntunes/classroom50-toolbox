// Errors cross IPC as `{ kind, message, params }`: a kind the catalog words is shown in the
// UI's language with its params; any other shows Rust's message (tool output, gh's words).
import { expect, test, type App } from "./app";
import { fail, ok, project, scenario, settings, snapshot } from "./fixtures";
import type { Reply } from "./fixtures";

const lab = project({ slug: "lab1", name: "Lab 1", pending: "unsubmitted" });
const clean = { hasMarker: true, uncommitted: [], editorFiles: [] };

async function submitFails(app: App, reply: Reply, language = "en") {
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [lab] }),
      settings: settings({ language }),
      responses: { submit_check: [ok(clean)], submit_project: [reply] },
    }),
  );
  await app.row("Lab 1").locator("button.primary").click();
  await app.answer("ok");
  return app.dialog();
}

test("an unknown kind shows Rust's message as is", async ({ app }) => {
  const dialog = await submitFails(app, fail("student", "gh-student: tag already exists"));
  expect(dialog.message).toBe("The submission failed: gh-student: tag already exists");
  expect(dialog.kind).toBe("error");
});

test("a known kind is worded by the catalog, params filled", async ({ app }) => {
  const dialog = await submitFails(
    app,
    fail("otherBranch", "Rust's English words", { branch: "main" }),
  );
  expect(dialog.message).toBe("The submission failed: the clone isn't on main");
});

test("a known kind missing a param falls back to Rust's message", async ({ app }) => {
  const dialog = await submitFails(app, fail("otherBranch", "the clone isn't on trunk"));
  expect(dialog.message).toBe("The submission failed: the clone isn't on trunk");
});

test("a known kind follows the UI's language", async ({ app }) => {
  const dialog = await submitFails(app, fail("offline", "can't reach GitHub"), "fr");
  // French puts a narrow no-break space before the colon.
  expect(dialog.message).toMatch(/^Le rendu a échoué\s:\simpossible de joindre GitHub$/);
});

test("a sync issue is worded by kind, Rust's message in its hint", async ({ app, page }) => {
  await app.start(
    scenario({
      snapshot: snapshot({
        projects: [lab],
        issue: { kind: "somethingNew", message: "GraphQL: Something went wrong", retryAfter: null },
      }),
    }),
  );
  const issue = page.locator(".issue");
  await expect(issue).toHaveText("Couldn't reach GitHub. Showing the last known state.");
  await expect(issue).toHaveAttribute("title", "GraphQL: Something went wrong");
});
