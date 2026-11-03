import { beforeEach, describe, expect, it } from "vitest";
import type { Project, Settings } from "#lib/api.js";
import { defaultEditor, editorFor, editors } from "#lib/editor.svelte.js";
import { settings } from "#lib/settings.svelte.js";

const idea = { id: "idea", name: "IntelliJ IDEA" };
const vscode = { id: "vscode", name: "Visual Studio Code" };

function project(editor: string | null, lastEditor: string | null = null): Project {
  return { editor, lastEditor } as Project;
}

describe("editor choice", () => {
  beforeEach(() => {
    editors.installed = [idea, vscode];
    settings.current = { defaultEditor: null } as Settings;
  });

  it("defaults to the first installed editor", () => {
    expect(defaultEditor()).toEqual(idea);
  });

  it("uses the chosen default when it's installed", () => {
    settings.current = { defaultEditor: "vscode" } as Settings;
    expect(defaultEditor()).toEqual(vscode);
    settings.current = { defaultEditor: "zed" } as Settings;
    expect(defaultEditor()).toEqual(idea);
  });

  it("opens a clone in the editor that last opened it", () => {
    expect(editorFor(project("vscode"))).toEqual(vscode);
  });

  it("prefers the editor last picked in the app", () => {
    expect(editorFor(project("idea", "vscode"))).toEqual(vscode);
    expect(editorFor(project(null, "vscode"))).toEqual(vscode);
  });

  it("skips a picked editor that is gone", () => {
    settings.current = { defaultEditor: "vscode" } as Settings;
    expect(editorFor(project("idea", "datagrip"))).toEqual(idea);
    expect(editorFor(project(null, "datagrip"))).toEqual(vscode);
  });

  it("falls back to the default when that editor is gone", () => {
    settings.current = { defaultEditor: "vscode" } as Settings;
    expect(editorFor(project("cursor"))).toEqual(vscode);
    expect(editorFor(project(null))).toEqual(vscode);
  });

  it("has nothing to offer with no editor installed", () => {
    editors.installed = [];
    expect(editorFor(project("idea"))).toBeNull();
  });
});
