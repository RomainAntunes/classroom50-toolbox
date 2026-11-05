<script lang="ts">
  import {
    APPEARANCES,
    COMMIT_ACTIONS,
    UPDATE_CHANNELS,
    type AppInfo,
    type Settings,
    withDialog,
  } from "#lib/api.js";
  import { atLeast } from "#lib/busy.js";
  import { languageName, languages, loadLanguage, resolveLanguage, t } from "#lib/i18n/index.js";
  import { CheckMenuItem, Menu } from "@tauri-apps/api/menu";
  import { message } from "@tauri-apps/plugin-dialog";
  import Spinner from "#lib/components/Spinner.svelte";
  import { chooseDefaultEditor, defaultEditor, editors } from "#lib/editor.svelte.js";
  import Section from "#lib/components/Section.svelte";
  import Toggle from "#lib/components/Toggle.svelte";
  import ShortcutRecorder from "#lib/components/ShortcutRecorder.svelte";
  import { popupMenu } from "#lib/keyboard.js";
  import { session } from "#lib/session.svelte.js";
  import { settings } from "#lib/settings.svelte.js";
  import { save } from "./save";

  let { current, info }: { current: Settings; info: AppInfo | null } = $props();

  let loadingLanguage = $state(false);

  /**
   * The picked language's catalog first, spinning meanwhile, then the setting, which the
   * layout applies at once; a catalog that won't load keeps the language shown.
   */
  async function pickLanguage(code: string) {
    loadingLanguage = true;
    let failed = false;
    try {
      await atLeast(loadLanguage(code));
    } catch (err) {
      console.error("couldn't load the catalog", err);
      failed = true;
    }
    if (!failed) await save({ language: code });
    loadingLanguage = false;
    if (failed) {
      const language = languageName(resolveLanguage(code));
      await withDialog(() =>
        message(t("settings.language.loadFailed", { language }), { kind: "error" }),
      );
    }
  }

  /** "System", then each catalog named in its own language, as a native menu. */
  async function chooseLanguage(anchor: HTMLElement) {
    const chosen = settings.current?.language ?? "system";
    const codes = ["system", ...Object.keys(languages)];
    const items = await Promise.all(
      codes.map((code) =>
        CheckMenuItem.new({
          text: code === "system" ? t("settings.language.system") : languageName(code),
          checked: code === chosen,
          action: () => void pickLanguage(code),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  /** Stable or Beta, as a native menu. */
  async function chooseUpdateChannel(anchor: HTMLElement) {
    const chosen = settings.current?.updateChannel ?? "stable";
    const items = await Promise.all(
      UPDATE_CHANNELS.map((choice) =>
        CheckMenuItem.new({
          text: t(`settings.updates.${choice}`),
          checked: choice === chosen,
          action: () => void save({ updateChannel: choice }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  /** Off, Commit or Commit & push, as a native menu. */
  async function chooseCommitActions(anchor: HTMLElement) {
    const chosen = settings.current?.commitActions ?? "none";
    const items = await Promise.all(
      COMMIT_ACTIONS.map((choice) =>
        CheckMenuItem.new({
          text: t(`settings.commitActions.${choice}`),
          checked: choice === chosen,
          action: () => void save({ commitActions: choice }),
        }),
      ),
    );
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }

  const editor = $derived(current ? defaultEditor() : null);

  const update = $derived(session.update);

  // MSI installs stay on stable (pre-releases ship no MSI).
  const channel = $derived(
    update?.hasBeta === false ? "stable" : (current?.updateChannel ?? "stable"),
  );
</script>

<Section title={t("settings.general.title")}>
  <div class="row">
    <span class="text">
      <span>{t("settings.editor.label")}</span>
      <span class="sub"
        >{t(editors.installed.length ? "settings.editor.hint" : "settings.editor.none")}</span
      >
    </span>
    {#if editor}
      <button
        aria-haspopup="menu"
        aria-label={`${t("settings.editor.label")}: ${editor.name}`}
        onclick={(event) => chooseDefaultEditor(event.currentTarget)}>{editor.name}</button
      >
    {/if}
  </div>
  <div class="row">
    <span class="text">
      <span>{t("settings.commitActions")}</span>
      <span class="sub">{t(`settings.commitActions.${current.commitActions}Hint`)}</span>
    </span>
    <button
      aria-haspopup="menu"
      aria-label={`${t("settings.commitActions")}: ${t(`settings.commitActions.${current.commitActions}`)}`}
      onclick={(event) => chooseCommitActions(event.currentTarget)}
      >{t(`settings.commitActions.${current.commitActions}`)}</button
    >
  </div>
  {#if current.hidden.length || current.hiddenClasses.length}
    <div class="row">
      <span class="text">
        <span>{t("settings.hidden.label")}</span>
        <span class="sub"
          >{t("settings.hidden.hint", {
            count: current.hidden.length,
            classes: current.hiddenClasses.length,
          })}</span
        >
      </span>
      <button onclick={() => save({ hidden: [], hiddenClasses: [] })}
        >{t("settings.hidden.showAll")}</button
      >
    </div>
  {/if}
  <div class="row">
    <span id="appearance-label">{t("settings.appearance")}</span>
    <div class="segmented" role="group" aria-labelledby="appearance-label">
      {#each APPEARANCES as choice (choice)}
        <button
          aria-pressed={current.appearance === choice}
          onclick={() => save({ appearance: choice })}>{t(`settings.appearance.${choice}`)}</button
        >
      {/each}
    </div>
  </div>
  <div class="row">
    <span>{t("settings.language")}</span>
    <button
      aria-haspopup="menu"
      aria-label={`${t("settings.language")}: ${
        current.language === "system"
          ? t("settings.language.system")
          : languageName(current.language)
      }`}
      disabled={loadingLanguage}
      aria-busy={loadingLanguage}
      onclick={(event) => chooseLanguage(event.currentTarget)}
      >{#if loadingLanguage}<Spinner />{/if}{current.language === "system"
        ? t("settings.language.system")
        : languageName(current.language)}</button
    >
  </div>
  {#if info}
    <ShortcutRecorder os={info.os} supported={info.globalShortcuts} />
  {/if}
  <Toggle
    label={t("settings.launchAtLogin")}
    hint={t("settings.launchAtLoginHint")}
    checked={current.launchAtLogin}
    onchange={(on) => save({ launchAtLogin: on })}
  />
  <div class="row">
    <span class="text">
      <span>{t("settings.updates.channel")}</span>
      <span class="sub"
        >{update?.hasBeta === false
          ? t("settings.updates.msiHint")
          : t(`settings.updates.${channel}Hint`)}</span
      >
    </span>
    <button
      aria-haspopup="menu"
      disabled={update?.hasBeta === false}
      aria-label={`${t("settings.updates.channel")}: ${t(`settings.updates.${channel}`)}`}
      onclick={(event) => chooseUpdateChannel(event.currentTarget)}
      >{t(`settings.updates.${channel}`)}</button
    >
  </div>
</Section>

<style>
  @import "./row.css";
  @import "./label.css";
  @import "./segmented.css";
</style>
