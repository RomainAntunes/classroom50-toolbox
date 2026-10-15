<script lang="ts">
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, withDialog, type Project } from "$lib/api";
  import { cloneProject } from "$lib/clone";
  import { submitProject } from "$lib/submit";
  import { durationLabel, formatDate, fromNow, t } from "$lib/i18n";
  import { tildify } from "$lib/paths.svelte";
  import Details from "./Details.svelte";

  let { project, now, unseen }: { project: Project; now: Date; unseen: boolean } = $props();

  const DAY = 24 * 60 * 60 * 1000;

  const name = $derived(project.assignment?.name ?? project.config.assignment);
  const closed = $derived(Boolean(project.assignment?.closed || project.remote?.archived));
  const due = $derived(project.assignment?.due ? new Date(project.assignment.due) : null);
  const dueSoon = $derived(
    due !== null && !closed && due > now && due.getTime() - now.getTime() < DAY,
  );
  const dueLabel = $derived(
    due === null
      ? null
      : t(due > now ? "projects.due" : "projects.wasDue", { when: fromNow(due, now) }),
  );
  const behind = $derived(project.local?.behind ?? 0);
  const defaultBranch = $derived(project.remote?.defaultBranch ?? "main");
  // Pushed and local branches holding work that isn't graded yet.
  const unmerged = $derived([
    ...new Set([
      ...(project.remote?.unmerged.map((b) => b.name) ?? []),
      ...(project.local?.unmerged ?? []),
    ]),
  ]);
  // One status at most, most important first; `warn` ones need the student's action.
  const grading = $derived(project.remote?.grading ?? null);
  const status = $derived.by(() => {
    if (closed) return { label: t("projects.closed"), warn: false, hint: undefined };
    // The last push isn't graded and won't be until it's pushed again or the teacher regrades.
    if (grading?.state === "failed") {
      return {
        label: t("projects.gradingFailed"),
        warn: true,
        hint: grading.message ?? t("projects.gradingFailedHint"),
      };
    }
    if (project.pending === "unmerged") {
      return {
        label: t("projects.pending.unmerged", { branch: defaultBranch }),
        warn: true,
        hint: t("projects.unmergedBranches", { branches: unmerged.join(", ") }),
      };
    }
    if (project.pending === "conflict") {
      return {
        label: t("projects.pending.conflict"),
        warn: true,
        hint: t("projects.conflictHint"),
      };
    }
    if (project.pending) {
      return { label: t(`projects.pending.${project.pending}`), warn: true, hint: undefined };
    }
    if (behind > 0) {
      return { label: t("projects.behind", { count: behind }), warn: false, hint: undefined };
    }
    if (project.local?.remoteAhead) {
      return { label: t("projects.pull"), warn: false, hint: t("projects.pullHint") };
    }
    return null;
  });
  const grade = $derived(project.remote?.grade ?? null);
  // A grade older than the head says nothing about it while the new one is on its way.
  const failedTests = $derived(
    grading?.state === "running" ? [] : (grade?.tests.filter((test) => !test.passed) ?? []),
  );
  const feedback = $derived(project.remote?.feedback ?? null);
  const path = $derived(project.path);
  const githubUrl = $derived(
    project.remote?.url ??
      (project.owner && project.repo
        ? `https://github.com/${project.owner}/${project.repo}`
        : null),
  );
  const openTarget = $derived(project.webUrl ?? githubUrl);
  const cloneable = $derived(!path && Boolean(project.owner && project.repo));

  const submittable = $derived(Boolean(path) && !closed);
  // Local work that isn't graded yet; other rows submit from the menu only.
  const ready = $derived(
    submittable &&
      (project.pending === "uncommitted" ||
        project.pending === "unpushed" ||
        project.pending === "unsubmitted"),
  );

  let submitting = $state(false);
  async function submit() {
    submitting = true;
    try {
      await submitProject(project, name);
    } finally {
      submitting = false;
    }
  }

  let cloning = $state(false);
  async function clone(choose = false) {
    cloning = true;
    try {
      await cloneProject(project.id, choose);
    } finally {
      cloning = false;
    }
  }

  let open = $state(false);
  const details = $derived.by(() => {
    const a = project.assignment;
    const remote = project.remote;
    const items: [string, string][] = [];
    const add = (key: string, value: string | null | undefined) => {
      if (value) items.push([t(`details.${key}`), value]);
    };
    // The class is the section title, and the repo is on the row and behind ↗.
    if (a) {
      add("type", t(`details.mode.${a.mode}`));
      add("submission", t(`details.submissionMode.${a.submissionMode ?? "every-push"}`));
      const opens = a.availableFrom ? new Date(a.availableFrom) : null;
      if (opens && opens > now) add("opens", formatDate(opens));
    }
    if (due) add("due", formatDate(due));
    if (remote?.head?.at) {
      const when = fromNow(new Date(remote.head.at), now);
      add("lastPush", remote.head.author ? `${remote.head.author}, ${when}` : when);
    }
    // With every-push grading the last submission is the last push.
    if (a?.submissionMode === "tag" && remote?.lastSubmission?.at) {
      add("lastSubmission", fromNow(new Date(remote.lastSubmission.at), now));
    }
    if (grading?.state === "running") add("grade", t("projects.grading"));
    else if (grade) {
      add(
        "grade",
        `${grade.score}/${grade.maxScore} · ${t("projects.gradeTests", { passed: grade.testsPassed, total: grade.testsTotal })}`,
      );
    }
    if (feedback?.comments) add("feedback", String(feedback.comments));
    if (unmerged.length) add("unmerged", unmerged.join(", "));
    if (project.local) {
      const local = project.local;
      // Only the default branch is graded, so another one is worth pointing out.
      if (local.branch !== defaultBranch) add("branch", local.branch);
      if (local.changedFiles) add("changes", String(local.changedFiles));
    }
    add("folder", path ? tildify(path) : null);
    return items;
  });

  /** The row's native context menu with every action. */
  async function showMenu(event: MouseEvent) {
    event.preventDefault();
    const os = document.documentElement.dataset.os ?? "linux";
    const separator = () => PredefinedMenuItem.new({ item: "Separator" });
    const items = await Promise.all([
      MenuItem.new({
        text: t("projects.openClassroom50"),
        enabled: Boolean(project.webUrl),
        action: () => project.webUrl && openUrl(project.webUrl),
      }),
      MenuItem.new({
        text: t("projects.openGitHub"),
        enabled: Boolean(githubUrl),
        action: () => githubUrl && openUrl(githubUrl),
      }),
      MenuItem.new({
        text: t("projects.openFeedback"),
        enabled: Boolean(feedback),
        action: () => feedback && openUrl(feedback.url),
      }),
      MenuItem.new({
        text: t("projects.openGrade"),
        enabled: Boolean(grade),
        action: () => grade && openUrl(grade.url),
      }),
      MenuItem.new({
        text: t("projects.openGradingRun"),
        enabled: Boolean(grading?.url),
        action: () => grading?.url && openUrl(grading.url),
      }),
      separator(),
      MenuItem.new({
        text: t("projects.submit"),
        enabled: submittable && !submitting,
        action: () => submit(),
      }),
      MenuItem.new({
        text: t("projects.clone"),
        enabled: cloneable && !cloning,
        action: () => clone(),
      }),
      MenuItem.new({
        text: t("projects.cloneTo"),
        enabled: cloneable && !cloning,
        action: () => clone(true),
      }),
      MenuItem.new({
        text: t(`projects.openFolder.${os}`, {}, "projects.openFolder.linux"),
        enabled: Boolean(path),
        action: () => path && revealItemInDir(path),
      }),
      MenuItem.new({
        text: t("projects.openTerminal"),
        enabled: Boolean(path),
        action: () => path && api.openTerminal(path),
      }),
    ]);
    const menu = await Menu.new({ items });
    // Pinned like a dialog, so the popover stays open while the menu is up.
    await withDialog(() => menu.popup());
  }
</script>

<div class="item">
  <div
    class="row"
    role="button"
    tabindex="0"
    aria-expanded={open}
    aria-label={name}
    oncontextmenu={showMenu}
    onclick={() => (open = !open)}
    onkeydown={(event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        open = !open;
      }
    }}
  >
    <div class="text">
      <span class="name" title={project.repo ?? path ?? undefined}>
        {#if unseen}<span class="dot" aria-label={t("projects.new")}></span>{/if}{name}
      </span>
      <span class="meta">
        {#if submitting}
          <span class="status">{t("projects.submitting")}</span> ·
        {:else if status}
          <span class="status" class:warn={status.warn} title={status.hint}>{status.label}</span> ·
        {/if}
        {#if feedback && feedback.comments > 0}
          <span class="feedback">{t("projects.comments", { count: feedback.comments })}</span> ·
        {/if}
        {#if dueLabel}
          <span class:soon={dueSoon}>{dueLabel}</span> ·
        {/if}
        {#if project.lateSeconds}
          <span class="late" title={t("projects.lateHint")}
            >{t("projects.late", { duration: durationLabel(project.lateSeconds) })}</span
          > ·
        {/if}
        {path
          ? (project.repo ?? project.config.assignment)
          : t(cloning ? "projects.cloning" : "projects.notCloned")}
      </span>
    </div>
    {#if grading?.state === "running"}
      <span class="grade grading" title={t("projects.gradingHint")}>{t("projects.grading")}</span>
    {:else if grade}
      <span
        class="grade"
        class:full={grade.score === grade.maxScore}
        title={t("projects.gradeTests", { passed: grade.testsPassed, total: grade.testsTotal })}
        >{grade.score}/{grade.maxScore}</span
      >
    {/if}
    {#if ready}
      <button
        class="primary"
        disabled={submitting}
        onclick={(event) => {
          event.stopPropagation();
          submit();
        }}>{t("submit.ok")}</button
      >
    {/if}
    {#if cloneable}
      <button
        title={t("projects.clone")}
        disabled={cloning}
        onclick={(event) => {
          event.stopPropagation();
          clone();
        }}>⤓</button
      >
    {/if}
    {#if openTarget}
      <button
        title={t(project.webUrl ? "projects.openClassroom50" : "projects.openGitHub")}
        onclick={(event) => {
          event.stopPropagation();
          openUrl(openTarget);
        }}>↗</button
      >
    {/if}
  </div>
  {#if open}
    <Details
      description={project.assignment?.description ?? null}
      items={details}
      failed={failedTests}
    />
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding: 8px 12px;
    transition: background-color 0.12s;
  }

  .row:hover {
    background: var(--bg-control);
  }

  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-weight: 500;
  }

  .meta {
    font-size: 0.85em;
    color: var(--fg-muted);
  }

  .soon {
    color: var(--warning);
  }

  .status {
    font-weight: 500;
  }

  .status.warn {
    color: var(--warning);
  }

  .late {
    color: var(--warning);
  }

  .feedback {
    color: var(--accent);
  }

  .grade {
    flex-shrink: 0;
    font-size: 0.85em;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }

  .grade.full {
    color: var(--success);
  }

  .grade.grading {
    font-weight: 500;
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }

  /* News since the popover was last open: a new grade, comment or assignment. */
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: 1px;
  }
</style>
