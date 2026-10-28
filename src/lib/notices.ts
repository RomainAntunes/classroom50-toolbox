import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { Notice } from "#lib/api.js";
import { formatDate, formatList, relativeAt, t } from "#lib/i18n/index.js";

/** Words a notice from Rust (`alerts.rs`) and shows it as a system notification. */
export async function showNotice(notice: Notice) {
  const { kind, ...params } = notice;
  const values: Record<string, string | number> = {};
  for (const [key, value] of Object.entries(params)) {
    if (typeof value === "string" || typeof value === "number") values[key] = value;
  }
  if (notice.kind === "groupJoined") {
    values.members = formatList(notice.members);
    values.count = notice.members.length;
  }
  if (notice.kind === "due") values.when = relativeAt(new Date(notice.due));
  if (notice.kind === "deadlineChanged") {
    values.when = notice.due ? formatDate(new Date(notice.due)) : t("alerts.noDeadline");
  }
  const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
  if (granted) {
    sendNotification({
      title: t(`alerts.${kind}.title`, values),
      body: t(`alerts.${kind}.body`, values),
    });
  }
}

const HOUR = 60 * 60 * 1000;

/** One made-up notice of each kind, for the dev build's notification tester. */
export function sampleNotices(now = Date.now()): Notice[] {
  const assignment = "Lab 1";
  return [
    { kind: "due", assignment, due: new Date(now + 6 * HOUR).toISOString() },
    { kind: "grade", assignment, score: 8, maxScore: 10 },
    { kind: "gradingFailed", assignment },
    { kind: "feedback", assignment, count: 2 },
    { kind: "newAssignment", assignment: "Lab 2", classroom: "CS50" },
    { kind: "teammatePush", assignment, author: "alice" },
    { kind: "groupJoined", assignment, members: ["Alice Martin"] },
    { kind: "deadlineChanged", assignment, due: new Date(now + 48 * HOUR).toISOString() },
    { kind: "closed", assignment },
    { kind: "updateReady", version: "0.11.0" },
  ];
}
