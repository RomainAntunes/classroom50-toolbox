<script lang="ts">
  /** The expanded part of a row: the assignment's description, then label/value lines. */
  let {
    description,
    items,
  }: { description: string | null; items: [label: string, value: string][] } = $props();
</script>

<div class="details">
  {#if description}
    <p class="description">{description}</p>
  {/if}
  <dl>
    {#each items as [label, value] (label)}
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    {/each}
  </dl>
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
</style>
