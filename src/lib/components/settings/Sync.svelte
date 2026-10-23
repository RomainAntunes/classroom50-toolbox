<script lang="ts">
  import { api, errorMessage, type LastRun, type SyncDetails } from "#lib/api.js";
  import { formatDate, formatNumber, fromNow, t } from "#lib/i18n/index.js";
  import Section from "#lib/components/Section.svelte";
  import Spinner from "#lib/components/Spinner.svelte";
  import { atLeast } from "#lib/busy.js";
  import { session } from "#lib/session.svelte.js";

  let { sync = $bindable() }: { sync: SyncDetails | null } = $props();

  let clearing = $state(false);
  // Diagnostics: folded away unless asked for.
  let showSync = $state(false);

  const rate = $derived(sync?.rateLimit ?? sync?.stats.rateLimit ?? null);

  function lastRun(run: LastRun | null): string {
    if (!run) return t("settings.sync.never");
    return t("settings.sync.run", {
      when: fromNow(new Date(run.at)),
      seconds: formatNumber(run.seconds, 1),
    });
  }

  async function clearCache() {
    clearing = true;
    try {
      session.snapshot = await atLeast(api.clearCache());
    } finally {
      clearing = false;
      sync = await api.syncDetails();
    }
  }
</script>

{#if sync}
  <Section title={t("settings.sync.title")}>
    {#snippet actions()}
      <button disabled={clearing} aria-busy={clearing} onclick={clearCache}
        >{#if clearing}<Spinner />{/if}{t("settings.sync.clear")}</button
      >
    {/snippet}
    <button class="disclosure" aria-expanded={showSync} onclick={() => (showSync = !showSync)}>
      <svg class="mirror-rtl" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg
      >
      {t(showSync ? "settings.sync.hide" : "settings.sync.show")}
    </button>
    {#if showSync}
      <dl class="sync">
        <dt>{t("settings.sync.account")}</dt>
        <dd><bdi>{sync.login ?? t("settings.sync.none")}</bdi></dd>
        <dt>{t("settings.sync.full")}</dt>
        <dd>{lastRun(sync.stats.full)}</dd>
        <dt>{t("settings.sync.status")}</dt>
        <dd>{lastRun(sync.stats.status)}</dd>
        <dt>{t("settings.sync.local")}</dt>
        <dd>{lastRun(sync.stats.local)}</dd>
        {#if rate}
          <dt>{t("settings.sync.budget")}</dt>
          <dd
            title={rate.resetAt
              ? t("settings.sync.resets", { when: formatDate(new Date(rate.resetAt)) })
              : undefined}
          >
            {t("settings.sync.budgetValue", { remaining: rate.remaining, count: rate.limit })}
          </dd>
        {/if}
        <dt>{t("settings.sync.found")}</dt>
        <dd>
          {t("settings.sync.foundValue", {
            repos: t("settings.sync.repos", { count: sync.stats.reposSeen }),
            assignments: t("settings.sync.assignments", { count: sync.assignments }),
            clones: t("settings.sync.cloned", { count: sync.clones }),
          })}
        </dd>
        <dt>{t("settings.sync.disk")}</dt>
        <dd>
          {t("settings.sync.diskValue", {
            folders: t("settings.sync.folders", { count: sync.stats.scanRoots }),
            watched: t("settings.sync.watched", { count: sync.stats.watchTargets }),
          })}
        </dd>
        {#if sync.issue}
          <dt>{t("settings.sync.issue")}</dt>
          <dd class="issue" title={errorMessage(sync.issue)}>
            <bdi>{errorMessage(sync.issue)}</bdi>
          </dd>
        {/if}
      </dl>
    {/if}
  </Section>
{/if}

<style>
  @import "./disclosure.css";

  .sync {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    margin: 0;
    padding: 10px 12px;
    font-size: 0.9em;
  }

  .sync dt {
    color: var(--fg-muted);
  }

  .sync dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sync .issue {
    color: var(--warning);
  }
</style>
