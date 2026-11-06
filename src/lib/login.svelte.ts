// Signing in through gh's device flow, kept here rather than in `LoginStatus` so the code
// still shows and works after the browser hid the popover (and the layout left Settings),
// and so Settings can run a grant too. The layout starts the listeners (`watchLogin`).
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  api,
  onEvent,
  DEVICE_CODE_EVENT,
  LOGIN_ENDED_EVENT,
  errorKind,
  errorMessage,
  type AuthStatus,
  type DeviceCode,
  type ExtraScope,
} from "#lib/api.js";
import { announce } from "#lib/announce.svelte.js";
import { rememberProtocol } from "#lib/links.js";
import { session } from "#lib/session.svelte.js";
import { t } from "#lib/i18n/index.js";

export const login = $state({
  pending: false,
  /** A step before or after gh's device flow: probing SSH, then adding the key. */
  step: null as "checking" | "settingUp" | null,
  device: null as DeviceCode | null,
  /** The browser couldn't be opened (Linux without `xdg-open`): the URL is spelled out. */
  openFailed: false,
  error: null as string | null,
});

function start() {
  login.pending = true;
  login.error = null;
  login.device = null;
  login.openFailed = false;
}

function end() {
  login.pending = false;
  login.device = null;
  login.step = null;
}

export function loginError(err: unknown): string {
  switch (errorKind(err)) {
    case "tokenFromEnv":
      return t("auth.envToken", { var: session.auth?.envToken ?? "GH_TOKEN" });
    case "timedOut":
      return t("auth.expired");
    default:
      return errorMessage(err);
  }
}

/** Signs in (or adds an account), first asking HTTPS or SSH (`ssh.ts`, loaded then). */
export async function signIn(onchange?: (status: AuthStatus) => void): Promise<void> {
  if (login.pending) return;
  start();
  try {
    const ssh = await import("#lib/ssh.js");
    const choice = await ssh.askSignInProtocol((checking) => {
      login.step = checking ? "checking" : null;
    });
    if (!choice) return;
    const status = await api.login(choice.protocol, choice.sshKey);
    session.auth = status;
    if (status.loggedIn) rememberProtocol(choice.protocol);
    onchange?.(status);
    await notifySignedIn(status);
    if (choice.sshKey && status.loggedIn) {
      login.device = null;
      login.step = "settingUp";
      await ssh.setUpAfterSignIn();
    }
  } catch (err) {
    if (errorKind(err) !== "cancelled") login.error = loginError(err);
  } finally {
    end();
  }
}

/** Adds scopes to the active account (gh-student's missing ones with no `extra`). Null
 *  when cancelled or failed (the footer shows why). */
export async function grant(
  extra: ExtraScope[] = [],
  onchange?: (status: AuthStatus) => void,
): Promise<AuthStatus | null> {
  if (login.pending) return null;
  start();
  try {
    const status = await api.grantScopes(extra);
    session.auth = status;
    onchange?.(status);
    await notifySignedIn(status);
    return status;
  } catch (err) {
    if (errorKind(err) !== "cancelled") login.error = loginError(err);
    return null;
  } finally {
    end();
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

/** Without a TTY gh never opens the browser itself, so the app does. */
export async function copyAndOpen() {
  const device = login.device;
  if (!device) return;
  await navigator.clipboard.writeText(device.code).catch(() => {});
  try {
    await openUrl(device.url);
  } catch {
    login.openFailed = true;
  }
}

/** Hides the code at once; Rust kills gh and `login-ended` follows. */
export function cancelLogin() {
  end();
  void api.cancelLogin();
}

let watching = false;

/** Starts following gh's device flow; later calls do nothing. */
export function watchLogin() {
  if (watching) return;
  watching = true;
  void onEvent(DEVICE_CODE_EVENT, (code) => {
    login.step = null;
    login.device = code;
    announce(`${t("auth.waiting")} ${code.code}`);
  });
  // Also resets a view that didn't start the sign-in (e.g. after a reload).
  void onEvent(LOGIN_ENDED_EVENT, () => {
    login.device = null;
    if (login.step !== "settingUp") login.pending = false;
  });
}
