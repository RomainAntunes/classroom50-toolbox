<script lang="ts">
  import { hoursLabel, t } from "#lib/i18n/index.js";
  import Section from "#lib/components/Section.svelte";
  import Toggle from "#lib/components/Toggle.svelte";
  import type { Settings } from "#lib/api.js";
  import { save } from "./save";

  let { current }: { current: Settings } = $props();

  const REMINDER_CHOICES = [72, 24, 6, 1];

  function toggleReminder(hours: number, on: boolean) {
    const rest = current?.notifyHoursBefore.filter((h) => h !== hours) ?? [];
    save({ notifyHoursBefore: on ? [...rest, hours] : rest });
  }
</script>

<Section title={t("settings.reminders.title")}>
  {#each REMINDER_CHOICES as hours (hours)}
    <Toggle
      label={t("settings.reminders.before", { duration: hoursLabel(hours) })}
      checked={current.notifyHoursBefore.includes(hours)}
      onchange={(on) => toggleReminder(hours, on)}
    />
  {/each}
</Section>
