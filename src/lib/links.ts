// What a row's "Copy" menu puts on the clipboard: the `classroom50-toolbox://` links (they
// must read back with Rust's `deeplink::parse`), the repo's GitHub page and its clone URLs.
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, type GitProtocol, type Project, type Unaccepted } from "$lib/api";

export const SCHEME = "classroom50-toolbox";

/** Starts accepting the assignment, with the unlisted classroom's key when it has one. */
export function acceptLink(item: Unaccepted): string {
  const { org, slug, secret } = item.classroom;
  const query = secret ? `?k=${secret}` : "";
  return `${SCHEME}://accept/${org}/${slug}/${item.assignment.slug}${query}`;
}

/** Shows the project; null for a clone with no GitHub repo. */
export function openLink(project: Project): string | null {
  return project.owner && project.repo ? `${SCHEME}://open/${project.owner}/${project.repo}` : null;
}

/** The ways to clone a GitHub repo, as the "Copy" menu lists them. */
export type CloneUrlKind = "https" | "ssh" | "gh";

/** `owner/repo`'s clone URLs and gh command, gh's `git_protocol` first. */
export function cloneUrls(
  owner: string,
  repo: string,
  protocol: GitProtocol,
): { kind: CloneUrlKind; text: string }[] {
  const https = { kind: "https" as const, text: `https://github.com/${owner}/${repo}.git` };
  const ssh = { kind: "ssh" as const, text: `git@github.com:${owner}/${repo}.git` };
  const gh = { kind: "gh" as const, text: `gh repo clone ${owner}/${repo}` };
  return protocol === "ssh" ? [ssh, https, gh] : [https, ssh, gh];
}

let protocol: GitProtocol | null = null;

/** gh's `git_protocol`, read once, then refreshed in the background so a menu opens at once
 *  and follows `gh config set` the time after. HTTPS when gh can't say. */
export async function gitProtocol(): Promise<GitProtocol> {
  const read = api.gitProtocol().then(
    (value) => (protocol = value),
    () => protocol ?? "https",
  );
  return protocol ?? read;
}

export function copyLink(link: string): Promise<void> {
  return writeText(link);
}
