import { describe, expect, it } from "vitest";
import { compare, formatBytes, formatTable, gzipSize, kindOf, measure } from "./size-budget.mjs";

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
});
