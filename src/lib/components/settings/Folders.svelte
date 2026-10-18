<script lang="ts">
  import { t } from "#lib/i18n/index.js";
  import { pickWatchedFolder } from "#lib/settings.svelte.js";
  import { pickCloneFolder } from "#lib/clone.js";
  import Section from "#lib/components/Section.svelte";
  import Toggle from "#lib/components/Toggle.svelte";
  import type { Settings } from "#lib/api.js";
  import { save } from "./save";

  let { current }: { current: Settings } = $props();

  function removeFolder(folder: string) {
    save({ watchedFolders: current?.watchedFolders.filter((f) => f !== folder) ?? [] });
  }
</script>

<Section title={t("settings.folders.title")}>
  <Toggle
    label={t("settings.folders.editors")}
    hint={t("settings.folders.editorsHint")}
    checked={current.scanEditorProjects}
    onchange={(on) => save({ scanEditorProjects: on })}
  />
  <div class="row">
    <span class="text">
      <span>{t("settings.folders.cloneInto")}</span>
      <span class="sub path" title={current.cloneFolder ?? undefined}
        >{#if current.cloneFolder}<bdi dir="ltr">{current.cloneFolder}</bdi>{:else}<bdi
            >{t("settings.folders.cloneUnset")}</bdi
          >{/if}</span
      >
    </span>
    <button onclick={pickCloneFolder}>{t("settings.folders.change")}</button>
  </div>
  <div class="row">
    <span class="text">
      <span>{t("settings.folders.watched")}</span>
      <span class="sub">{t("settings.folders.hint")}</span>
    </span>
    <button onclick={pickWatchedFolder}>{t("projects.addFolder")}</button>
  </div>
  {#each current.watchedFolders as folder (folder)}
    <div class="row nested">
      <span class="path" title={folder}><bdi dir="ltr">{folder}</bdi></span>
      <button onclick={() => removeFolder(folder)}>{t("settings.folders.remove")}</button>
    </div>
  {/each}
</Section>

<style>
  @import "./row.css";
  @import "./label.css";
  @import "./path.css";

  .row.nested {
    padding-inline-start: 24px;
  }
</style>
