import { describe, expect, it } from "vitest";
import {
  compare,
  componentOf,
  formatBytes,
  formatTable,
  gzipSize,
  kindOf,
  measure,
  startupFiles,
  startupRoots,
} from "./size-budget.mjs";

const file = (path, text) => ({ path, bytes: new TextEncoder().encode(text) });
const budget = { js: { raw: 100, gzip: 100 }, css: { raw: 10, gzip: 100 } };

describe("size budget", () => {
  it("counts JavaScript and CSS only", () => {
    expect(kindOf("_app/immutable/chunks/a.js")).toBe("js");
    expect(kindOf("worker.mjs")).toBe("js");
    expect(kindOf("_app/immutable/assets/2.css")).toBe("css");
    for (const path of ["index.html", "_app/version.json", "logo.png", "a.js.map", "a.cssx"]) {
      expect(kindOf(path)).toBeNull();
    }
  });

  it("sums raw and gzipped bytes per kind", () => {
    const totals = measure([
      file("a.js", "let a = 1;"),
      file("b.js", "x".repeat(1000)),
      file("app.css", "body{}"),
      file("index.html", "<html>".repeat(50)),
    ]);
    expect(totals.js.files).toBe(2);
    expect(totals.js.raw).toBe(1010);
    // Repetitive text compresses far below its size.
    expect(totals.js.gzip).toBeLessThan(200);
    expect(totals.js.gzip).toBe(
      gzipSize(file("", "let a = 1;").bytes) + gzipSize(file("", "x".repeat(1000)).bytes),
    );
    expect(totals.css).toEqual({ files: 1, raw: 6, gzip: gzipSize(file("", "body{}").bytes) });
  });

  it("flags what goes over its limit", () => {
    const rows = compare({ js: { raw: 100, gzip: 30 }, css: { raw: 11, gzip: 20 } }, budget);
    expect(rows.map((row) => `${row.kind}.${row.metric}:${row.ok}`)).toEqual([
      "js.raw:true",
      "js.gzip:true",
      "css.raw:false",
      "css.gzip:true",
    ]);
  });

  it("refuses a budget without a limit", () => {
    expect(() => compare(measure([]), { js: { raw: 1, gzip: 1 }, css: { raw: 1 } })).toThrow(
      "css.gzip",
    );
  });

  it("formats sizes and the table", () => {
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(29_410)).toBe("29.4 kB");
    expect(formatBytes(4_466_028)).toBe("4.47 MB");
    // Halves round up, whatever their binary value.
    expect(formatBytes(1_045_000)).toBe("1.05 MB");
    expect(formatBytes(29_450)).toBe("29.5 kB");
    expect(formatBytes(999_960)).toBe("1.00 MB");
    const table = formatTable(
      compare({ js: { raw: 50, gzip: 30 }, css: { raw: 20, gzip: 5 } }, budget),
    );
    expect(table.split("\n")).toEqual([
      "| Bundle | Size | Limit | Used |",
      "| --- | ---: | ---: | ---: |",
      "| JS (raw) | 50 B | 100 B | 50% |",
      "| JS (gzip) | 30 B | 100 B | 30% |",
      "| CSS (raw) | 20 B | 10 B | **200%**, over |",
      "| CSS (gzip) | 5 B | 100 B | 5% |",
    ]);
  });

  describe("at startup", () => {
    const NODES = ".svelte-kit/generated/build/client-optimized/nodes";
    // Shaped like Vite's client manifest for a SvelteKit app: Kit's entries, a node per
    // layout and page, shared chunks, and chunks only imported dynamically.
    const manifest = {
      "node_modules/@sveltejs/kit/src/runtime/client/entry.js": {
        file: "_app/immutable/entry/start.js",
        isEntry: true,
        imports: ["_shared.js"],
        dynamicImports: ["node_modules/@sveltejs/kit/src/runtime/client/client-entry.js"],
      },
      ".svelte-kit/generated/build/client-optimized/app.js": {
        file: "_app/immutable/entry/app.js",
        isEntry: true,
        imports: ["_shared.js"],
        dynamicImports: [`${NODES}/0.js`, `${NODES}/2.js`, `${NODES}/3.js`],
      },
      "node_modules/@sveltejs/kit/src/runtime/client/client-entry.js": {
        file: "_app/immutable/chunks/client.js",
        isDynamicEntry: true,
      },
      [`${NODES}/0.js`]: {
        file: "_app/immutable/nodes/0.js",
        isEntry: true,
        imports: ["_shared.js", "_i18n.js"],
        css: ["_app/immutable/assets/0.css"],
      },
      [`${NODES}/2.js`]: {
        file: "_app/immutable/nodes/2.js",
        isEntry: true,
        imports: ["_i18n.js", "_list.js"],
        css: ["_app/immutable/assets/2.css"],
      },
      [`${NODES}/3.js`]: {
        file: "_app/immutable/nodes/3.js",
        isEntry: true,
        imports: ["_settings.js"],
        css: ["_app/immutable/assets/3.css"],
      },
      "_shared.js": { file: "_app/immutable/chunks/shared.js" },
      "_i18n.js": {
        file: "_app/immutable/chunks/i18n.js",
        dynamicImports: ["src/lib/i18n/fr.json"],
      },
      "_list.js": {
        file: "_app/immutable/chunks/list.js",
        imports: ["_shared.js"],
        css: ["_app/immutable/assets/list.css"],
      },
      "_settings.js": { file: "_app/immutable/chunks/settings.js" },
      "src/lib/i18n/fr.json": { file: "_app/immutable/chunks/fr.js", isDynamicEntry: true },
    };
    const components = {
      [`${NODES}/0.js`]: "src/routes/+layout.svelte",
      [`${NODES}/2.js`]: "src/routes/+page.svelte",
      [`${NODES}/3.js`]: "src/routes/settings/+page.svelte",
    };

    it("reads the component a generated node exports", () => {
      const key = `${NODES}/2.js`;
      const page = 'export { default as component } from "../../../../../src/routes/+page.svelte";';
      expect(componentOf(key, page)).toBe("src/routes/+page.svelte");
      const layout =
        'import * as universal from "../../../../../src/routes/+layout.ts";\n' +
        "export { universal };\n" +
        "export { default as component } from '../../../../../src/routes/+layout.svelte';";
      expect(componentOf(key, layout)).toBe("src/routes/+layout.svelte");
      expect(componentOf(key, "export const nodes = [];")).toBeNull();
    });

    it("starts from Kit's entries, the root layout and the main page", () => {
      const roots = startupRoots(manifest, (key) => components[key] ?? null);
      expect(roots).toEqual([
        "node_modules/@sveltejs/kit/src/runtime/client/entry.js",
        ".svelte-kit/generated/build/client-optimized/app.js",
        `${NODES}/0.js`,
        `${NODES}/2.js`,
      ]);
    });

    it("follows static imports and their CSS, never dynamic ones", () => {
      const roots = startupRoots(manifest, (key) => components[key] ?? null);
      expect(startupFiles(manifest, roots)).toEqual([
        "_app/immutable/assets/0.css",
        "_app/immutable/assets/2.css",
        "_app/immutable/assets/list.css",
        "_app/immutable/chunks/i18n.js",
        "_app/immutable/chunks/list.js",
        "_app/immutable/chunks/shared.js",
        "_app/immutable/entry/app.js",
        "_app/immutable/entry/start.js",
        "_app/immutable/nodes/0.js",
        "_app/immutable/nodes/2.js",
      ]);
    });

    it("refuses an import the manifest doesn't hold", () => {
      expect(() => startupFiles({ a: { file: "a.js", imports: ["_gone.js"] } }, ["a"])).toThrow(
        "the manifest has no _gone.js",
      );
    });
  });
});
