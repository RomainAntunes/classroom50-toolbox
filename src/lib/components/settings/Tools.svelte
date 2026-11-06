<script lang="ts">
  import { type ToolStatus } from "#lib/api.js";
  import { t } from "#lib/i18n/index.js";
  import Section from "#lib/components/Section.svelte";
  import ToolRow from "#lib/components/ToolRow.svelte";
  import GitLogin from "#lib/components/settings/GitLogin.svelte";
  import GitSsh from "#lib/components/settings/GitSsh.svelte";
  import Spinner from "#lib/components/Spinner.svelte";
  import { session } from "#lib/session.svelte.js";

  let { checking, oncheck }: { checking: boolean; oncheck: () => void } = $props();

  /** Rows still asking GitHub for a newer version: Check again spins until they're done. */
  let lookups = $state<Record<string, boolean>>({});
  const busy = $derived(checking || Object.values(lookups).some(Boolean));
  /** How missing tools install: folded, since the rows already offer Install. */
  let showInstalls = $state(false);

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
  <div class="intro">
    <p class="hint">{t("settings.tools.hint")}</p>
    <button
      class="disclosure"
      aria-expanded={showInstalls}
      aria-controls="tool-installs"
      onclick={() => (showInstalls = !showInstalls)}
    >
      <svg class="mirror-rtl" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg
      >
      {t("settings.tools.howInstalls")}
    </button>
    <p class="hint installs" id="tool-installs" hidden={!showInstalls}>
      {t("settings.tools.installs")}
    </p>
  </div>
  {#each session.tools as status (status.tool)}
    <ToolRow
      {status}
      onchange={replaceTool}
      checkUpdates
      onlookup={(busy) => (lookups[status.tool] = busy)}
    />
  {/each}
  <GitLogin />
  <GitSsh />
</Section>

<style>
  @import "./hint.css";
  @import "./disclosure.css";

  .intro {
    padding-block: 10px 4px;
  }

  .intro .hint {
    padding-block: 0;
    font-size: 0.85em;
  }

  .intro .disclosure {
    padding-block: 6px;
    font-size: 0.85em;
  }

  .installs {
    padding-block-end: 8px;
  }

  .installs[hidden] {
    display: none;
  }
</style>
