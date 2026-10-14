<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
  } from "@tauri-apps/plugin-notification";
  import {
    api,
    DEVICE_CODE_EVENT,
    errorKind,
    errorMessage,
    type AuthStatus,
    type DeviceCode,
  } from "$lib/api";
  import { t } from "$lib/i18n";

  let {
    auth,
    ghInstalled,
    onchange,
  }: { auth: AuthStatus | null; ghInstalled: boolean; onchange: (a: AuthStatus) => void } =
    $props();

  let pending = $state(false);
  let device = $state<DeviceCode | null>(null);
  let error = $state<string | null>(null);

  async function signIn() {
    pending = true;
    error = null;
    device = null;
    try {
      const status = await api.login();
      onchange(status);
      await notifySignedIn(status);
    } catch (err) {
      if (errorKind(err) !== "cancelled") error = errorMessage(err);
    } finally {
      pending = false;
      device = null;
    }
  }

  // The browser takes focus during the device flow, which hides the popover, so the result
  // is reported through a system notification.
  async function notifySignedIn(status: AuthStatus) {
    if (!status.loggedIn) return;
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) {
      sendNotification({
        title: t("auth.notification.title"),
        body: t("auth.notification.body", { login: status.login ?? "" }),
      });
    }
  }

  async function copyAndOpen() {
    if (!device) return;
    await navigator.clipboard.writeText(device.code).catch(() => {});
    await openUrl(device.url);
  }

  onMount(() => {
    const unlisten = listen<DeviceCode>(DEVICE_CODE_EVENT, (event) => (device = event.payload));
    return () => void unlisten.then((stop) => stop());
  });
</script>

{#if auth?.loggedIn}
  <span>{t("auth.loggedInAs", { login: auth.login ?? "" })}</span>
{:else if device}
  <div class="device">
    <span>{t("auth.waiting")}</span>
    <code>{device.code}</code>
    <div class="actions">
      <button class="primary" onclick={copyAndOpen}>{t("auth.copyAndOpen")}</button>
      <button onclick={() => api.cancelLogin()}>{t("auth.cancel")}</button>
    </div>
  </div>
{:else if auth}
  <div class="row">
    <span>{ghInstalled ? t("auth.loggedOut") : t("auth.needsGh")}</span>
    <button class="primary" disabled={!ghInstalled || pending} onclick={signIn}>
      {t("auth.signIn")}
    </button>
  </div>
  {#if error}
    <p class="error">{error}</p>
  {/if}
{/if}

<style>
  .row,
  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space);
  }

  .device {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  code {
    font-family: ui-monospace, "SF Mono", "Cascadia Mono", "DejaVu Sans Mono", monospace;
    font-size: 1.6em;
    letter-spacing: 0.08em;
    color: var(--fg);
    -webkit-user-select: all;
    user-select: all;
  }

  .error {
    margin: 4px 0 0;
    color: var(--danger);
  }
</style>
