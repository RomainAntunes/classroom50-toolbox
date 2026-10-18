<script lang="ts">
  import { type ToolStatus } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import Section from "#lib/components/Section.svelte";
  import ToolRow from "#lib/components/ToolRow.svelte";
  import { session } from "#lib/session.svelte.js";

  let { checking, oncheck }: { checking: boolean; oncheck: () => void } = $props();

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }
</script>

<Section title={t("settings.tools.title")}>
  {#snippet actions()}
    <button disabled={checking} onclick={oncheck}>{t("setup.recheck")}</button>
  {/snippet}
  <p class="hint">{t("settings.tools.hint")}</p>
  {#each session.tools as status (status.tool)}
    <ToolRow {status} onchange={replaceTool} checkUpdates />
  {/each}
</Section>

<style>
  @import "./hint.css";
</style>
