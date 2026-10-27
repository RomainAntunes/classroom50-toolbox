<script lang="ts">
  import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type Project } from "#lib/api.js";
  import { cloneProject, type CloneMode } from "#lib/clone.js";
  import { editorFor, editors, openInEditor } from "#lib/editor.svelte.js";
  import { pullProject } from "#lib/pull.js";
  import { commitLocal, commitPush, gradedOnPush } from "#lib/push.js";
  import { submitProject } from "#lib/submit.js";
  import { addTeammate, joinsGroup } from "#lib/teammate.js";
  import { isHidden, setHidden } from "#lib/hidden.js";
  import { durationLabel, formatDate, fromNow, isolated, t } from "#lib/i18n/index.js";
  import { tildify } from "#lib/paths.svelte.js";
  import { groupDetails } from "#lib/group.js";
  import { session } from "#lib/session.svelte.js";
  import { settings } from "#lib/settings.svelte.js";
  import { cloneUrls, copyLink, gitProtocol, openLink } from "#lib/links.js";
  import { menuKey, onContextMenu, popupMenu } from "#lib/keyboard.js";
  import { scrollBehavior } from "#lib/motion.js";
  import { changedLines } from "#lib/files.js";
  import { atLeast } from "#lib/busy.js";
  import Details from "./Details.svelte";
  import Spinner from "./Spinner.svelte";

  let { project, now, unseen }: { project: Project; now: Date; unseen: boolean } = $props();
  const uid = $props.id();

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
  // Graded on every push, and GitHub's head is the clone's HEAD with nothing left to push.
  const handedIn = $derived(
    gradedOnPush(project) &&
      Boolean(project.local?.head) &&
      project.remote?.head?.oid === project.local?.head &&
      !project.local?.ahead,
  );
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
    // Worded by what stops the app's Pull, which only fast-forwards over untracked files.
    if (project.pending === "conflict") {
      const [label, hint] = project.local?.ahead
        ? ["projects.pullAndMerge", "projects.pullAndMergeHint"]
        : project.local?.trackedChanges
          ? ["projects.commitThenPull", "projects.commitThenPullHint"]
          : ["projects.pending.conflict", "projects.conflictHint"];
      return { label: t(label), warn: true, hint: t(hint) };
    }
    // Every push is graded and GitHub has the clone's HEAD: the hand-in is done, and the
    // edits only exist on this computer.
    if (project.pending === "uncommitted" && project.local && handedIn) {
      return {
        label: t("projects.localChanges"),
        warn: false,
        hint: t("projects.localChangesHint", { files: changedLines(project.local) }),
      };
    }
    if (project.pending === "uncommitted" && project.local) {
      return {
        label: t("projects.pending.uncommitted"),
        warn: true,
        hint: t("projects.uncommittedHint", { files: changedLines(project.local) }),
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
  // Grades before the current one, newest first.
  const earlier = $derived((project.remote?.history ?? []).filter((g) => g.tag !== grade?.tag));
  const gradeLine = (g: { score: number; maxScore: number; publishedAt: string | null }) =>
    g.publishedAt
      ? t("details.gradeAt", {
          score: `${g.score}/${g.maxScore}`,
          when: fromNow(new Date(g.publishedAt), now),
        })
      : `${g.score}/${g.maxScore}`;
  const path = $derived(project.path);
  const githubUrl = $derived(
    project.remote?.url ??
      (project.owner && project.repo
        ? `https://github.com/${project.owner}/${project.repo}`
        : null),
  );
  const openTarget = $derived(project.webUrl ?? githubUrl);
  const hidden = $derived(isHidden(project.id));
  const link = $derived(openLink(project));
  // The editor that last opened the clone, else the default one.
  const editor = $derived(path ? editorFor(project) : null);
  const cloneable = $derived(!path && Boolean(project.owner && project.repo));

  const submittable = $derived(Boolean(path) && !closed);
  // Committed work that isn't graded yet; other rows submit from the menu only, which
  // explains why a clone with uncommitted changes can't submit.
  const ready = $derived(
    submittable &&
      (project.local?.changedFiles ?? 0) === 0 &&
      (project.pending === "unpushed" || project.pending === "unsubmitted"),
  );

  // Every-push assignments commit and push from the app; tag-mode ones submit. The
  // `commitActions` setting may leave either to git and the editor.
  const commitActions = $derived(settings.current?.commitActions ?? "none");
  const everyPush = $derived(gradedOnPush(project));
  const offersPush = $derived(everyPush && commitActions === "push");
  // No row button: the row shows where the work stands and the student picks Commit or
  // Commit & push from the menu, which explains a refusal.
  const pushable = $derived(submittable && offersPush);

  // GitHub has commits the clone lacks. The button only shows when a fast-forward can work
  // (untracked files don't stop one); the menu always offers it and explains a refusal.
  const incoming = $derived(Boolean(path) && (behind > 0 || Boolean(project.local?.remoteAhead)));
  const pullable = $derived(
    incoming &&
      project.local?.branch === defaultBranch &&
      project.local.trackedChanges === 0 &&
      !project.local.ahead,
  );

  let pulling = $state(false);
  async function pull() {
    pulling = true;
    try {
      await atLeast(pullProject(project, name));
    } finally {
      pulling = false;
    }
  }

  // Set once the student picks an action in the prompt, so the status names what runs.
  let pushing = $state(false);
  let committing = $state(false);
  let busy = $state(false);
  async function runCommit(run: typeof commitPush) {
    busy = true;
    try {
      await run(project, name, (push) => {
        pushing = push;
        committing = !push;
      });
    } finally {
      busy = pushing = committing = false;
    }
  }
  const push = () => runCommit(commitPush);
  const commit = () => runCommit(commitLocal);

  // A local commit works on any clone with changes, whatever the branch or submission mode.
  const committable = $derived(Boolean(path) && (project.local?.changedFiles ?? 0) > 0);

  let submitting = $state(false);
  /** The button spins while the check and gh-student run, not behind the questions. */
  let submitWorking = $state(false);
  async function submit() {
    submitting = true;
    try {
      await submitProject(project, name, (on) => (submitWorking = on));
    } finally {
      submitting = submitWorking = false;
    }
  }

  const cloning = $derived(session.cloning.includes(project.id));
  function clone(mode: CloneMode = "ask") {
    void cloneProject(project.id, mode);
  }

  let open = $state(false);

  // A link pointed here: open the details and bring the row into view.
  let element: HTMLElement | undefined = $state();
  const linked = $derived(session.focus?.id === project.id ? session.focus.seq : null);
  $effect(() => {
    if (linked === null) return;
    open = true;
    element?.scrollIntoView({ block: "nearest", behavior: scrollBehavior() });
  });
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
      add("lastPush", remote.head.author ? `${isolated(remote.head.author)}, ${when}` : when);
    }
    // With every-push grading the last submission is the last push.
    if (a?.submissionMode === "tag" && remote?.lastSubmission?.at) {
      add("lastSubmission", fromNow(new Date(remote.lastSubmission.at), now));
    }
    if (grading?.state === "running") add("grade", t("projects.grading"));
    else if (grade) {
      add(
        "grade",
        `${grade.score}/${grade.maxScore} · ${t("projects.gradeTests", { passed: grade.testsPassed, count: grade.testsTotal })}`,
      );
    }
    if (earlier.length) add("earlierGrades", earlier.map(gradeLine).join("\n"));
    if (feedback?.comments) add("feedback", String(feedback.comments));
    if (project.group) items.push(...groupDetails(project.group));
    if (unmerged.length) add("unmerged", unmerged.join(", "));
    if (project.local) {
      const local = project.local;
      // Only the default branch is graded, so another one is worth pointing out.
      if (local.branch !== defaultBranch) add("branch", local.branch);
      if (local.changedFiles) add("changes", changedLines(local, 10));
    }
    add("folder", path ? tildify(path) : null);
    return items;
  });

  /** The "Copy" submenu: the app link, the GitHub page and the clone URLs, gh's
   *  `git_protocol` first. Empty for a clone with no GitHub repo. */
  async function copyItems() {
    const { owner, repo } = project;
    if (!link || !githubUrl || !owner || !repo) return [];
    const urls = cloneUrls(owner, repo, await gitProtocol());
    return Promise.all([
      MenuItem.new({ text: t("copy.appLink"), action: () => copyLink(link) }),
      MenuItem.new({ text: t("copy.github"), action: () => copyLink(githubUrl) }),
      ...urls.map((url) =>
        MenuItem.new({ text: t(`copy.${url.kind}`), action: () => copyLink(url.text) }),
      ),
    ]);
  }

  /** The row's native context menu with every action, under `anchor` or at the pointer. */
  async function showMenu(anchor?: HTMLElement) {
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
        text: t("projects.openGroup"),
        enabled: Boolean(project.group),
        action: () => project.group && openUrl(project.group.url),
      }),
      MenuItem.new({
        text: t("projects.openGrade"),
        enabled: Boolean(grade),
        action: () => grade && openUrl(grade.url),
      }),
      Submenu.new({
        text: t("projects.earlierGrades"),
        enabled: earlier.length > 0,
        items: await Promise.all(
          earlier.map((g) => MenuItem.new({ text: gradeLine(g), action: () => openUrl(g.url) })),
        ),
      }),
      MenuItem.new({
        text: t("projects.openGradingRun"),
        enabled: Boolean(grading?.url),
        action: () => grading?.url && openUrl(grading.url),
      }),
      separator(),
      MenuItem.new({
        text: t("projects.pullAction"),
        enabled: incoming && !pulling,
        action: () => pull(),
      }),
      ...(commitActions !== "none"
        ? [
            MenuItem.new({
              text: t("commit.menu"),
              enabled: committable && !busy,
              action: () => commit(),
            }),
          ]
        : []),
      ...(offersPush
        ? [
            MenuItem.new({
              text: t("push.menu"),
              enabled: pushable && !busy,
              action: () => push(),
            }),
          ]
        : []),
      MenuItem.new({
        text: t("projects.submit"),
        enabled: submittable && !submitting,
        action: () => submit(),
      }),
      MenuItem.new({
        text: t(joinsGroup(project) ? "teammate.teamMenu" : "teammate.inviteMenu"),
        enabled: Boolean(project.owner && project.repo) && !closed,
        action: () => addTeammate(project, name),
      }),
      MenuItem.new({
        text: t("projects.clone"),
        enabled: cloneable && !cloning,
        action: () => clone(),
      }),
      MenuItem.new({
        text: t("projects.cloneTo"),
        enabled: cloneable && !cloning,
        action: () => clone("choose"),
      }),
      MenuItem.new({
        text: editor ? t("editor.open", { editor: editor.name }) : t("editor.none"),
        enabled: Boolean(path && editor),
        action: () => path && editor && openInEditor(path, editor),
      }),
      Submenu.new({
        text: t("editor.openWith"),
        enabled: Boolean(path) && editors.installed.length > 1,
        items: await Promise.all(
          editors.installed.map((other) =>
            MenuItem.new({
              text: other.name,
              action: () => path && openInEditor(path, other),
            }),
          ),
        ),
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
      separator(),
      Submenu.new({
        text: t("copy.menu"),
        enabled: link !== null,
        items: await copyItems(),
      }),
      MenuItem.new({
        text: t(hidden ? "projects.unhide" : "projects.hide"),
        action: () => setHidden(project.id, !hidden),
      }),
    ]);
    const menu = await Menu.new({ items });
    // Pinned like a dialog, so the popover stays open while the menu is up.
    await popupMenu(menu, anchor);
  }
</script>

<li class="item" class:linked={linked !== null} data-item bind:this={element}>
  <!-- A right click anywhere on the row opens its menu; the keyboard opens it from the
       row's button (Menu key or Shift+F10). -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="row" oncontextmenu={onContextMenu(() => showMenu())}>
    <button
      class="toggle"
      data-row
      aria-expanded={open}
      aria-labelledby="{uid}-name"
      aria-describedby={grade || grading?.state === "running"
        ? `${uid}-meta ${uid}-grade`
        : `${uid}-meta`}
      onclick={() => (open = !open)}
      onkeydown={menuKey((anchor) => showMenu(anchor))}
    >
      <span class="text">
        <span class="name" id="{uid}-name" title={project.repo ?? path ?? undefined}>
          {#if unseen}<span class="dot" aria-hidden="true"></span>{/if}<bdi>{name}</bdi
          >{#if unseen}<span class="sr-only">, {t("projects.new")}</span>{/if}
        </span>
        <span class="meta" id="{uid}-meta">
          {#if hidden}
            <span class="hidden-tag">{t("projects.hidden")}</span> ·
          {/if}
          {#if submitting}
            <span class="status">{t("projects.submitting")}</span> ·
          {:else if pushing}
            <span class="status">{t("push.pushing")}</span> ·
          {:else if committing}
            <span class="status">{t("commit.committing")}</span> ·
          {:else if pulling}
            <span class="status">{t("projects.pulling")}</span> ·
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
          {#if path}<bdi>{project.repo ?? project.config.assignment}</bdi>{:else}{t(
              cloning ? "projects.cloning" : "projects.notCloned",
            )}{/if}
        </span>
      </span>
      {#if grading?.state === "running"}
        <span class="grade grading" id="{uid}-grade" title={t("projects.gradingHint")}
          >{t("projects.grading")}</span
        >
      {:else if grade}
        <span
          class="grade"
          class:full={grade.score === grade.maxScore}
          id="{uid}-grade"
          title={t("projects.gradeTests", { passed: grade.testsPassed, count: grade.testsTotal })}
          ><span aria-hidden="true">{grade.score}/{grade.maxScore}</span><span class="sr-only"
            >{t("projects.gradeSr", { score: grade.score, max: grade.maxScore })}</span
          ></span
        >
      {/if}
    </button>
    {#if pullable}
      <button title={t("projects.pullHint")} disabled={pulling} aria-busy={pulling} onclick={pull}
        >{#if pulling}<Spinner />{/if}{t("projects.pullAction")}</button
      >
    {/if}
    {#if ready && !everyPush}
      <button class="primary" disabled={submitting} aria-busy={submitWorking} onclick={submit}
        >{#if submitWorking}<Spinner />{/if}{t("submit.ok")}</button
      >
    {/if}
    {#if cloneable}
      <button
        title={t("projects.clone")}
        aria-label={t("projects.clone")}
        disabled={cloning}
        aria-busy={cloning}
        onclick={() => clone()}
        >{#if cloning}<Spinner alone />{:else}<span aria-hidden="true">⤓</span>{/if}</button
      >
    {/if}
    {#if openTarget}
      {@const openLabel = t(project.webUrl ? "projects.openClassroom50" : "projects.openGitHub")}
      <button title={openLabel} aria-label={openLabel} onclick={() => openUrl(openTarget)}
        ><span class="mirror-rtl" aria-hidden="true">↗</span></button
      >
    {/if}
  </div>
  {#if open}
    <Details
      description={project.assignment?.description ?? null}
      items={details}
      failed={failedTests}
      notes={grading?.state === "running" ? null : (grade?.releaseNotes ?? null)}
      gradeUrl={grade?.url ?? null}
    />
  {/if}
</li>

<style>
  /* The row a link pointed to. */
  .item.linked {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  :global(:root[dir="rtl"]) .item.linked {
    box-shadow: inset -3px 0 0 var(--accent);
  }

  .item {
    list-style: none;
  }

  /* The button that opens the details fills the row up to its action buttons, so a click
     anywhere on the text toggles them as before. */
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding-inline-end: 12px;
    transition: background-color 0.12s;
  }

  .row:hover {
    background: var(--bg-control);
  }

  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding-block: 8px;
    padding-inline: 12px 0;
    border-radius: 0;
    background: none;
    text-align: start;
    white-space: normal;
  }

  .toggle:hover:not(:disabled) {
    background: none;
  }

  .toggle:focus-visible {
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

  .hidden-tag {
    font-style: italic;
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
    color: var(--accent-text);
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
    margin-inline-end: 6px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: 1px;
  }
</style>
