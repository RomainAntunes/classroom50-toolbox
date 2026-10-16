// Which way a language is written.

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
