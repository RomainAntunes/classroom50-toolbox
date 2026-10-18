// The installs and updates running, mirrored from Rust (which owns them) so a row shows
// one still going after the popover was hidden or the page changed.
import { listen } from "@tauri-apps/api/event";
import { api, TOOL_JOBS_EVENT, type Tool, type ToolJob } from "#lib/api.js";
import { session } from "#lib/session.svelte.js";

export const toolJobs = $state<{ running: Partial<Record<Tool, ToolJob>> }>({ running: {} });

let watching = false;

function apply(jobs: ToolJob[]) {
  const ended = Object.keys(toolJobs.running).some((tool) => !jobs.some((j) => j.tool === tool));
  toolJobs.running = Object.fromEntries(jobs.map((job) => [job.tool, job]));
  // The row that started it may be gone (another page); every view reads `session.tools`.
  if (ended) {
    void api
      .toolStatuses()
      .then((tools) => (session.tools = tools))
      .catch(() => {});
  }
}

/** Starts following the jobs; later calls do nothing. */
export function watchToolJobs() {
  if (watching) return;
  watching = true;
  void listen<ToolJob[]>(TOOL_JOBS_EVENT, (event) => apply(event.payload));
  void api
    .toolJobs()
    .then(apply)
    .catch(() => {});
}
