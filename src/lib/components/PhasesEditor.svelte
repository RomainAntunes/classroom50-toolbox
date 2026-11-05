<script lang="ts">
  // Rendered once in the layout; a fresh form for each project opened (`editPhases`). The
  // form is its own chunk, loaded the first time it opens, so it costs nothing at startup.
  import { phasesEditor } from "#lib/phasesEditor.svelte.js";

  const form = () => import("./PhasesForm.svelte");
</script>

{#if phasesEditor.current}
  {@const current = phasesEditor.current}
  {#await form() then { default: PhasesForm }}
    {#key current}
      <PhasesForm project={current.project} resolve={current.resolve} />
    {/key}
  {/await}
{/if}
