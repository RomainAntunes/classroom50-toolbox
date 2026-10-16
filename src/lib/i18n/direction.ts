// Which way a language is written, and how data keeps its own order inside a sentence.

export type Direction = "ltr" | "rtl";

// Languages written right to left in their usual script (`iw` is Hebrew's old code).
const RTL_LANGUAGES = new Set(["ar", "ckb", "dv", "fa", "he", "iw", "ps", "sd", "ug", "ur", "yi"]);
// Scripts written right to left, for tags that name one (`az-Arab`, `pa-Arab`).
const RTL_SCRIPTS = new Set(["adlm", "arab", "hebr", "nkoo", "rohg", "syrc", "thaa"]);

/**
 * `rtl` for a language written right to left (`ar`, `he`, `fa`, `ur`... with any region,
 * `ar-EG`, `fa_IR`), else `ltr`. A script subtag wins over the language (`sd-Deva` is
 * `ltr`, `az-Arab` is `rtl`).
 */
export function textDirection(code: string): Direction {
  // POSIX spellings too: `fa_IR.UTF-8`.
  const tag = code.trim().split(/[.@]/)[0].replace(/_/g, "-").toLowerCase();
  const [language, ...rest] = tag.split("-");
  const script = rest.find((part) => /^[a-z]{4}$/.test(part));
  if (script) return RTL_SCRIPTS.has(script) ? "rtl" : "ltr";
  return RTL_LANGUAGES.has(language) ? "rtl" : "ltr";
}

const FSI = "\u2068";
const PDI = "\u2069";

/**
 * `value` between FIRST STRONG ISOLATE and POP DIRECTIONAL ISOLATE: it takes the direction
 * of its own first letter and can't reorder the sentence around it, so `repo-name.` or
 * `~/code/cs50` stays as typed inside a right-to-left sentence.
 */
export function isolate(value: string): string {
  return `${FSI}${value}${PDI}`;
}
