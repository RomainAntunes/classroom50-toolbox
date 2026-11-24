<script lang="ts">
  // Rendered once in the layout; a fresh form each time one opens (`editManual`). The form is
  // its own chunk, loaded the first time it opens, so it costs nothing at startup.
  import { manualEditor } from "#lib/manualEditor.svelte.js";

  const form = () => import("./ManualProjectForm.svelte");
</script>

{#if manualEditor.current}
  {@const current = manualEditor.current}
  {#await form() then { default: ManualProjectForm }}
    {#key current}
      <ManualProjectForm init={current.init} resolve={current.resolve} />
    {/key}
  {/await}
{/if}
