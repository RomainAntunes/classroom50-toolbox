// The classmates being added, mirrored from Rust (which runs them in the background) so a
// row shows where each username stands after the popover was hidden or the page changed.
import { api, onEvent, TEAMMATE_JOBS_EVENT, type TeammateJob } from "#lib/api.js";
import { announce } from "#lib/announce.svelte.js";
import { resultsText } from "#lib/teammate.js";

export const teammateJobs = $state<{ jobs: TeammateJob[] }>({ jobs: [] });

let watching = false;

function apply(jobs: TeammateJob[]) {
  // A queue that just emptied: screen readers hear how it went (failures included, since
  // no dialog says it).
  for (const job of jobs) {
    const before = teammateJobs.jobs.find((other) => other.id === job.id);
    if (before?.running && !job.running) {
      const settled = job.entries.filter((e) => e.state === "added" || e.state === "failed");
      if (settled.length) announce(resultsText(settled, job));
    }
  }
  teammateJobs.jobs = jobs;
}

/** The queue shown on the row of `id`: queued from it, or from the assignment it was
 *  accepted from. */
export function jobFor(id: string): TeammateJob | null {
  return teammateJobs.jobs.find((job) => job.id === id || job.project === id) ?? null;
}

/** Starts following the queues; later calls do nothing. */
export function watchTeammateJobs() {
  if (watching) return;
  watching = true;
  void onEvent(TEAMMATE_JOBS_EVENT, apply);
  void api
    .teammateJobs()
    .then(apply)
    .catch(() => {});
}
