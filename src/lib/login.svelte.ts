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

/** Which view started a flow: its error shows there only (the footer, or Settings > SSH). */
export type LoginFrom = "footer" | "settings";

export const login = $state({
  pending: false,
  /** Who started the running flow (the device code shows there too). */
  from: "footer" as LoginFrom,
  /** A step before or after gh's device flow: probing SSH, then adding the key. */
  step: null as "checking" | "settingUp" | null,
  device: null as DeviceCode | null,
  /** The browser couldn't be opened (Linux without `xdg-open`): the URL is spelled out. */
  openFailed: false,
  error: null as { text: string; from: LoginFrom } | null,
});

// The running flow's id (0: none), so only its own end clears the state: `login-ended`
// and a cancel come before the flow's own `finally`.
let running = 0;
let flows = 0;

function start(from: LoginFrom): number {
  if (running) return 0;
  running = ++flows;
  login.pending = true;
  login.from = from;
  login.error = null;
  login.device = null;
  login.openFailed = false;
  return running;
}

function end(id: number) {
  if (id !== running) return;
  running = 0;
  login.pending = false;
  login.device = null;
  login.step = null;
}

function failed(err: unknown, from: LoginFrom) {
  if (errorKind(err) !== "cancelled") login.error = { text: loginError(err), from };
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

/** Signs in (or adds an account), first asking HTTPS or SSH (`ssh.ts`, loaded then); with
 *  SSH, checks it again once signed in, as that account. Ignored while a flow runs. */
export async function signIn(onchange?: (status: AuthStatus) => void): Promise<void> {
  const env = session.auth?.envToken;
  if (env && !running) {
    // gh can't store a sign-in: refused before anything is asked or fixed.
    login.error = { text: t("auth.envToken", { var: env }), from: "footer" };
    return;
  }
  const id = start("footer");
  if (!id) return;
  try {
    const ssh = await import("#lib/ssh.js");
    const choice = await ssh.askSignInProtocol((checking) => {
      login.step = checking ? "checking" : null;
    });
    if (!choice || id !== running) return;
    const status = await api.login(choice.protocol, choice.sshKey);
    session.auth = status;
    if (status.loggedIn) rememberProtocol(choice.protocol);
    onchange?.(status);
    await notifySignedIn(status);
    if (choice.protocol === "ssh" && status.loggedIn && id === running) {
      login.device = null;
      await ssh.afterSignIn(choice.sshKey, {
        step: (step) => (login.step = step),
        grant: () =>
          scopes(["writePublicKey"], onchange).catch((err) => (failed(err, "footer"), null)),
      });
    }
  } catch (err) {
    failed(err, "footer");
  } finally {
    end(id);
  }
}

async function scopes(
  extra: ExtraScope[],
  onchange?: (status: AuthStatus) => void,
): Promise<AuthStatus> {
  const status = await api.grantScopes(extra);
  session.auth = status;
  onchange?.(status);
  await notifySignedIn(status);
  return status;
}

/** Adds scopes to the active account (gh-student's missing ones with no `extra`). Null
 *  when cancelled or failed (the view that asked, `from`, shows why) or another flow runs. */
export async function grant(
  extra: ExtraScope[] = [],
  onchange?: (status: AuthStatus) => void,
  from: LoginFrom = "footer",
): Promise<AuthStatus | null> {
  const id = start(from);
  if (!id) return null;
  try {
    return await scopes(extra, onchange);
  } catch (err) {
    failed(err, from);
    return null;
  } finally {
    end(id);
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

/** Hides the code at once and ends the flow; Rust kills gh and `login-ended` follows. */
export function cancelLogin() {
  end(running);
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
  // Comes before the flow's own end: only resets a view that didn't start the sign-in
  // (e.g. after a reload).
  void onEvent(LOGIN_ENDED_EVENT, () => {
    login.device = null;
    if (!running) login.pending = false;
  });
}
