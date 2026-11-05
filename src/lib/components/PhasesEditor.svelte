<script lang="ts">
  // Rendered once in the layout; a fresh form for each project opened (`editPhases`).
  import { phasesEditor } from "#lib/phasesEditor.svelte.js";
  import PhasesForm from "./PhasesForm.svelte";
</script>

{#if phasesEditor.current}
  {#key phasesEditor.current}
    <PhasesForm project={phasesEditor.current.project} resolve={phasesEditor.current.resolve} />
  {/key}
{/if}
