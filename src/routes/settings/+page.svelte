<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { message } from "@tauri-apps/plugin-dialog";
  import { exit } from "@tauri-apps/plugin-process";
  import { api, errorMessage, withDialog, type AppInfo } from "$lib/api";
  import { hoursLabel, t } from "$lib/i18n";
  import { loadSettings, pickWatchedFolder, saveSettings, settings } from "$lib/settings.svelte";
  import Section from "$lib/components/Section.svelte";
  import Toggle from "$lib/components/Toggle.svelte";

  const REMINDER_CHOICES = [72, 24, 6, 1];

  let info = $state<AppInfo | null>(null);
  let checking = $state(false);
  let upToDate = $state(false);

  const current = $derived(settings.current);

  async function save(patch: Parameters<typeof saveSettings>[0]) {
    try {
      await saveSettings(patch);
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  function removeFolder(folder: string) {
    save({ watchedFolders: current?.watchedFolders.filter((f) => f !== folder) ?? [] });
  }

  function toggleReminder(hours: number, on: boolean) {
    const rest = current?.notifyHoursBefore.filter((h) => h !== hours) ?? [];
    save({ notifyHoursBefore: on ? [...rest, hours] : rest });
  }

  async function checkUpdates() {
    checking = true;
    upToDate = false;
    try {
      const update = await api.checkUpdate();
      if (update) await api.installUpdate();
      else upToDate = true;
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    } finally {
      checking = false;
    }
  }

  onMount(async () => {
    [info] = await Promise.all([api.appInfo(), loadSettings()]);
  });
</script>

<header class="titlebar">
  <button onclick={() => goto("/")} title={t("app.back")}>‹</button>
  <h1>{t("app.settings")}</h1>
</header>

{#if current}
  <main>
    <Section title={t("settings.folders.title")}>
      {#snippet actions()}
        <button onclick={pickWatchedFolder}>{t("projects.addFolder")}</button>
      {/snippet}
      {#each current.watchedFolders as folder (folder)}
        <div class="row">
          <span class="path" title={folder}>{folder}</span>
          <button onclick={() => removeFolder(folder)}>{t("settings.folders.remove")}</button>
        </div>
      {:else}
        <p class="hint">{t("settings.folders.hint")}</p>
      {/each}
    </Section>

    <Section title={t("settings.general.title")}>
      <Toggle
        label={t("settings.launchAtLogin")}
        checked={current.launchAtLogin}
        onchange={(on) => save({ launchAtLogin: on })}
      />
    </Section>

    <Section title={t("settings.reminders.title")}>
      {#each REMINDER_CHOICES as hours (hours)}
        <Toggle
          label={t("settings.reminders.before", { duration: hoursLabel(hours) })}
          checked={current.notifyHoursBefore.includes(hours)}
          onchange={(on) => toggleReminder(hours, on)}
        />
      {/each}
    </Section>

    <Section title={t("settings.about.title")}>
      <div class="row">
        <span>{info ? t("settings.about.version", { version: info.version }) : ""}</span>
        <button disabled={checking} onclick={checkUpdates}>
          {upToDate ? t("settings.about.upToDate") : t("settings.about.checkUpdates")}
        </button>
      </div>
    </Section>

    <button class="quit" onclick={() => exit(0)}>{t("settings.quit")}</button>
  </main>
{/if}

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: var(--space);
    padding: 12px 14px 4px;
  }

  h1 {
    margin: 0;
    font-size: 1.1em;
    font-weight: 600;
  }

  main {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: var(--space) 12px 14px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
    padding: 8px 12px;
  }

  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .hint {
    margin: 0;
    padding: 12px;
    color: var(--fg-muted);
  }

  .quit {
    align-self: center;
    color: var(--danger);
    background: transparent;
  }
</style>
