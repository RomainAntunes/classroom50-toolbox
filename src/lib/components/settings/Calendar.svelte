<script lang="ts">
  import { message, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import {
    api,
    errorMessage,
    withDialog,
    type AppInfo,
    type NativeCalendar,
    type Settings,
  } from "#lib/api.js";
  import { hasKey, t } from "#lib/i18n/index.js";
  import { loadSettings, settings } from "#lib/settings.svelte.js";
  import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
  import Section from "#lib/components/Section.svelte";
  import { popupMenu } from "#lib/keyboard.js";
  import { save } from "./save";

  let {
    current,
    info,
    calendars = $bindable(),
    denied = $bindable(false),
  }: {
    current: Settings;
    info: AppInfo | null;
    calendars: NativeCalendar[];
    /** macOS denies calendar access, turned off since the calendar was chosen. */
    denied?: boolean;
  } = $props();

  let exported = $state<number | null>(null);
  async function exportCalendar(): Promise<boolean> {
    const path = await withDialog(() =>
      saveDialog({
        defaultPath: "classroom50.ics",
        filters: [{ name: t("settings.calendar.fileType"), extensions: ["ics"] }],
      }),
    );
    if (!path) return false;
    try {
      exported = await api.exportCalendar(path);
      // Rust now keeps that file updated (`calendarFile`).
      await loadSettings();
      return true;
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
      return false;
    }
  }

  // No calendar store to write into (Windows, Linux without GNOME's or KDE's): the calendar
  // app imports the file, and importing it again updates the events (stable UIDs).
  async function openInCalendar() {
    if (!settings.current?.calendarFile && !(await exportCalendar())) return;
    try {
      await api.openCalendarFile();
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  // Worded per kind; anything else (a D-Bus call that failed on Linux) adds the details.
  async function calendarError(err: unknown) {
    const key = `calendar.error.${(err as { kind?: string }).kind}`;
    const text = hasKey(key) ? t(key) : `${t("calendar.error.fallback")}\n\n${errorMessage(err)}`;
    await withDialog(() => message(text, { kind: "error" }));
  }

  /** "Classroom 50 (GNOME Calendar)" on Linux, "Work (iCloud)" on macOS. */
  function calendarLabel(calendar: NativeCalendar): string {
    const account = calendar.kind
      ? t(`settings.calendar.store.${calendar.kind}`)
      : calendar.account;
    return account ? `${calendar.title} (${account})` : calendar.title;
  }

  let writing = $state(false);
  const chosenCalendar = $derived(
    calendars.find((c) => c.id === settings.current?.nativeCalendar) ?? null,
  );

  async function openPrivacy() {
    try {
      await api.openCalendarPrivacy();
    } catch (err) {
      await withDialog(() => message(errorMessage(err), { kind: "error" }));
    }
  }

  async function setNativeCalendar(id: string | null) {
    writing = true;
    try {
      settings.current = await api.setNativeCalendar(id);
    } catch (err) {
      await calendarError(err);
    } finally {
      writing = false;
    }
  }

  async function chooseCalendar(anchor: HTMLElement) {
    try {
      // The first call raises macOS's access prompt, which takes focus: pinned, the popover
      // stays on Settings for the menu that follows.
      calendars = await withDialog(() => api.nativeCalendars());
      denied = false;
    } catch (err) {
      await calendarError(err);
      return;
    }
    const current = settings.current?.nativeCalendar ?? null;
    const items: (MenuItem | PredefinedMenuItem)[] = await Promise.all(
      calendars.map((calendar) =>
        MenuItem.new({
          text: calendarLabel(calendar),
          enabled: calendar.id !== current,
          action: () => setNativeCalendar(calendar.id),
        }),
      ),
    );
    if (current) {
      items.push(
        await PredefinedMenuItem.new({ item: "Separator" }),
        await MenuItem.new({
          text: t("settings.calendar.stopSync"),
          action: () => setNativeCalendar(null),
        }),
      );
    }
    const menu = await Menu.new({ items });
    await popupMenu(menu, anchor);
  }
</script>

<Section title={t("settings.calendar.title")}>
  {#if info?.nativeCalendar}
    <div class="row">
      <span class="text">
        <span>{t("settings.calendar.app")}</span>
        <span class="sub"
          >{current.nativeCalendar
            ? t("settings.calendar.syncing", {
                calendar: chosenCalendar
                  ? calendarLabel(chosenCalendar)
                  : t("settings.calendar.unknown"),
              })
            : t("settings.calendar.appHint")}</span
        >
      </span>
      <button
        disabled={writing}
        aria-haspopup="menu"
        onclick={(event) => chooseCalendar(event.currentTarget)}
        >{t(
          current.nativeCalendar ? "settings.calendar.change" : "settings.calendar.choose",
        )}</button
      >
    </div>
    {#if denied && current.nativeCalendar}
      <div class="row">
        <span class="text denied">{t("settings.calendar.denied")}</span>
        <button onclick={openPrivacy}>{t("settings.calendar.openPrivacy")}</button>
      </div>
    {/if}
  {/if}
  <div class="row">
    <span class="text">
      <span>{t("settings.calendar.label")}</span>
      <span class="sub"
        >{exported === null
          ? t("settings.calendar.hint")
          : t("settings.calendar.done", { count: exported })}</span
      >
    </span>
    <button onclick={exportCalendar}>{t("settings.calendar.export")}</button>
  </div>
  {#if info && !info.nativeCalendar}
    <div class="row">
      <span class="text">
        <span>{t("settings.calendar.openLabel")}</span>
        <span class="sub">{t("settings.calendar.openHint")}</span>
      </span>
      <button onclick={openInCalendar}>{t("settings.calendar.open")}</button>
    </div>
  {/if}
  {#if current.calendarFile}
    <div class="row">
      <span class="text">
        <span>{t("settings.calendar.kept")}</span>
        <span class="sub path" title={current.calendarFile}
          ><bdi dir="ltr">{current.calendarFile}</bdi></span
        >
      </span>
      <button onclick={() => save({ calendarFile: null })}>{t("settings.calendar.stop")}</button>
    </div>
  {/if}
</Section>

<style>
  @import "./row.css";
  @import "./label.css";
  @import "./path.css";

  .denied {
    color: var(--warning);
  }
</style>
