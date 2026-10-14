<script lang="ts">
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type LocalProject } from "$lib/api";
  import { t } from "$lib/i18n";

  let { project }: { project: LocalProject } = $props();
</script>

<div class="row">
  <div class="text">
    <span class="name">{project.config.assignment}</span>
    <span class="meta">
      {project.owner ?? "?"} · {project.config.classroom}
    </span>
  </div>
  <button title={t("projects.openFolder")} onclick={() => revealItemInDir(project.path)}>
    ⧉
  </button>
  <button title={t("projects.openTerminal")} onclick={() => api.openTerminal(project.path)}>
    &gt;_
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) / 2);
    padding: 8px 12px;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: 0.85em;
    color: var(--fg-muted);
  }
</style>
