// The `classroom50-toolbox://` links a row's "Copy link" shares. They must read back with
// Rust's `deeplink::parse`.
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Project, Unaccepted } from "$lib/api";

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

export function copyLink(link: string): Promise<void> {
  return writeText(link);
}
