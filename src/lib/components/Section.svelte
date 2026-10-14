<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, actions, children }: { title: string; actions?: Snippet; children: Snippet } =
    $props();
</script>

<section>
  <header>
    <h2>{title}</h2>
    {@render actions?.()}
  </header>
  <div class="card">
    {@render children()}
  </div>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 0.75);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
  }

  h2 {
    margin: 0;
    font-size: 0.85em;
    font-weight: 600;
    color: var(--fg-muted);
  }

  .card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    overflow: hidden;
  }

  .card > :global(* + *) {
    border-top: 1px solid var(--border);
  }
</style>
