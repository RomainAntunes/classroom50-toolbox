// Where a tool's version links to in the Tools rows: the release notes on GitHub.
import type { Tool } from "#lib/api.js";

/** `1.58.2` or `v1.58.2`, nothing else, since it goes into a URL. */
const VERSION = /^v?(\d+\.\d+\.\d+)$/;

/** The page with `version`'s notes, or null. gh-student's own releases only link a
 * compare, so its notes are the `cli-v*` releases of the Classroom 50 repo it's built in;
 * git has no GitHub releases. */
export function releaseNotesUrl(tool: Tool, version: string | null): string | null {
  const number = version?.trim().match(VERSION)?.[1];
  if (!number) return null;
  switch (tool) {
    case "gh":
      return `https://github.com/cli/cli/releases/tag/v${number}`;
    case "gh-student":
      return `https://github.com/foundation50/classroom50/releases/tag/cli-v${number}`;
    case "git":
      return null;
  }
}
