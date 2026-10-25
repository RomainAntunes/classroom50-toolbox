<script lang="ts">
  import { type ToolStatus } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import Section from "#lib/components/Section.svelte";
  import ToolRow from "#lib/components/ToolRow.svelte";
  import GitLogin from "#lib/components/settings/GitLogin.svelte";
  import Spinner from "#lib/components/Spinner.svelte";
  import { session } from "#lib/session.svelte.js";

  let { checking, oncheck }: { checking: boolean; oncheck: () => void } = $props();

  /** Rows still asking GitHub for a newer version: Check again spins until they're done. */
  let lookups = $state<Record<string, boolean>>({});
  const busy = $derived(checking || Object.values(lookups).some(Boolean));

  function replaceTool(next: ToolStatus) {
    session.tools = session.tools.map((tool) => (tool.tool === next.tool ? next : tool));
  }
</script>

<Section title={t("settings.tools.title")}>
  {#snippet actions()}
    <button disabled={busy} aria-busy={busy} onclick={oncheck}
      >{#if busy}<Spinner />{/if}{t("setup.recheck")}</button
    >
  {/snippet}
  <p class="hint">{t("settings.tools.hint")}</p>
  {#each session.tools as status (status.tool)}
    <ToolRow
      {status}
      onchange={replaceTool}
      checkUpdates
      onlookup={(busy) => (lookups[status.tool] = busy)}
    />
  {/each}
  <GitLogin />
</Section>

<style>
  @import "./hint.css";
</style>
