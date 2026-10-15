// Which rows the list shows: the date filter, the search, and hidden assignments (only shown
// when a search matches them). `query` is trimmed and lowercased by the caller.
import type { ListFilter, Project, Unaccepted } from "$lib/api";

const WEEK = 7 * 24 * 60 * 60 * 1000;

type Fields = (string | null | undefined)[];

/** Whether an assignment with this due date and state passes the date filter. */
export function inRange(filter: ListFilter, due: string | null, closed: boolean, now: Date) {
  const at = due ? new Date(due).getTime() : null;
  const ahead = at !== null && at > now.getTime();
  switch (filter) {
    case "open":
      return !closed && (at === null || ahead);
    case "week":
      return !closed && ahead && at - now.getTime() <= WEEK;
    case "past":
      return closed || (at !== null && !ahead);
    default:
      return true;
  }
}

function matches(fields: Fields, query: string): boolean {
  return !query || fields.some((field) => field?.toLowerCase().includes(query));
}

// Teammates too, so "Maendly" finds the labs shared with them.
function groupFields(group: Project["group"]): Fields {
  return [group?.name, ...(group?.members.flatMap((m) => [m.login, m.name]) ?? [])];
}

export interface ListQuery {
  filter: ListFilter;
  query: string;
  hidden: string[];
  now: Date;
}

export function showsProject(project: Project, { filter, query, hidden, now }: ListQuery) {
  const fields = [
    project.assignment?.name,
    project.config.assignment,
    project.config.classroom,
    project.classroom?.name,
    project.owner,
    project.repo,
    ...groupFields(project.group),
  ];
  const closed = Boolean(project.assignment?.closed || project.remote?.archived);
  return (
    (!hidden.includes(project.id) || query !== "") &&
    matches(fields, query) &&
    inRange(filter, project.assignment?.due ?? null, closed, now)
  );
}

export function showsUnaccepted(item: Unaccepted, { filter, query, hidden, now }: ListQuery) {
  const fields = [
    item.assignment.name,
    item.assignment.slug,
    item.classroom.slug,
    item.classroom.name,
    item.classroom.org,
    ...groupFields(item.group),
  ];
  return (
    (!hidden.includes(item.id) || query !== "") &&
    matches(fields, query) &&
    inRange(filter, item.assignment.due, item.assignment.closed, now)
  );
}
