<script lang="ts">
  import type { GradeTest } from "$lib/api";
  import { t } from "$lib/i18n";

  /** Failed tests listed before "and N more". */
  const MAX_TESTS = 8;

  /**
   * The expanded part of a row: the assignment's description, label/value lines, then the
   * tests the last grade failed.
   */
  let {
    description,
    items,
    failed = [],
  }: {
    description: string | null;
    items: [label: string, value: string][];
    failed?: GradeTest[];
  } = $props();
</script>

<div class="details">
  {#if description}
    <p class="description">{description}</p>
  {/if}
  <dl>
    {#each items as [label, value] (label)}
      <dt>{label}</dt>
      <dd title={value} class:lines={value.includes("\n")}>{value}</dd>
    {/each}
  </dl>
  {#if failed.length}
    <p class="heading">{t("details.failedTests")}</p>
    <ul class="tests">
      {#each failed.slice(0, MAX_TESTS) as test, i (i)}
        <li>
          <span class="name" title={test.name}>{test.name}</span>
          <span class="points">{test.score}/{test.maxScore}</span>
        </li>
      {/each}
    </ul>
    {#if failed.length > MAX_TESTS}
      <p class="more">{t("details.moreTests", { count: failed.length - MAX_TESTS })}</p>
    {/if}
  {/if}
</div>

<style>
  .details {
    padding: 0 12px 10px;
    font-size: 0.85em;
    /* Selectable, unlike the rest of the app, so a path or branch can be copied. */
    -webkit-user-select: text;
    user-select: text;
    cursor: auto;
  }

  .description {
    margin: 0 0 8px;
    color: var(--fg-muted);
    white-space: pre-line;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 3px 12px;
    margin: 0;
  }

  dt {
    color: var(--fg-muted);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* One entry per line, such as a group's members. */
  dd.lines {
    white-space: pre-line;
  }

  .heading {
    margin: 8px 0 3px;
    color: var(--fg-muted);
  }

  .tests {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tests li {
    display: flex;
    gap: 12px;
  }

  .tests .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tests .points {
    color: var(--warning);
    font-variant-numeric: tabular-nums;
  }

  .more {
    margin: 3px 0 0;
    color: var(--fg-muted);
  }
</style>
