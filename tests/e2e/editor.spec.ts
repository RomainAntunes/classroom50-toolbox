// Opening a clone in an editor from a row's menu: the editor picked in "Open with" is
// remembered (Rust sends the list again with the project's `lastEditor`), so the row's
// "Open in" and the tray's Open Recent offer it from then on.
import { expect, menuTexts, test } from "./app";
import { HOME, project, scenario, settings, snapshot } from "./fixtures";

const EDITORS = [
  { id: "vscode", name: "Visual Studio Code" },
  { id: "idea", name: "IntelliJ IDEA" },
  { id: "datagrip", name: "DataGrip" },
];
const HELLO = project({ slug: "hello", name: "Hello", editor: "idea" });
const PATH = `${HOME}/Classroom/cs101-hello-student`;

test("the editor picked in Open with becomes the row's Open in", async ({ app }) => {
  await app.start(scenario({ snapshot: snapshot({ projects: [HELLO] }), editors: EDITORS }));

  // The editor the editors' recent lists name comes first.
  expect(menuTexts(await app.rowMenu("Hello"))).toContain("Open in IntelliJ IDEA");
  await app.pickMenu("DataGrip");
  const [call] = await app.waitForCall("open_in_editor");
  expect(call.args).toEqual({ path: PATH, editor: "datagrip" });

  // Remembered: the next menu offers it, and its Open in opens it there.
  await expect
    .poll(async () => menuTexts(await app.rowMenu("Hello")))
    .toContain("Open in DataGrip");
  await app.pickMenu("Open in DataGrip");
  const calls = await app.waitForCall("open_in_editor", 2);
  expect(calls[1].args).toEqual({ path: PATH, editor: "datagrip" });

  // The tray's Open Recent follows it too.
  await app.emit("tray-action", { kind: "open", id: HELLO.id });
  const all = await app.waitForCall("open_in_editor", 3);
  expect(all[2].args).toEqual({ path: PATH, editor: "datagrip" });
});

test("the French menu names the remembered editor", async ({ app }) => {
  const picked = project({ slug: "hello", name: "Hello", editor: "idea", lastEditor: "datagrip" });
  await app.start(
    scenario({
      snapshot: snapshot({ projects: [picked] }),
      editors: EDITORS,
      settings: settings({ language: "fr" }),
    }),
  );
  expect(menuTexts(await app.rowMenu("Hello"))).toContain("Ouvrir dans DataGrip");
});

test("a remembered editor that is gone falls back to the detected one", async ({ app }) => {
  const picked = project({ slug: "hello", name: "Hello", editor: "idea", lastEditor: "zed" });
  await app.start(scenario({ snapshot: snapshot({ projects: [picked] }), editors: EDITORS }));
  const texts = menuTexts(await app.rowMenu("Hello"));
  expect(texts).toContain("Open in IntelliJ IDEA");
  expect(await app.calls("open_in_editor")).toEqual([]);
});
