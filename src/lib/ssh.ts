// HTTPS or SSH with GitHub: the sign-in question, SSH's status in words, its fixes (GitHub's
// host keys, a key on the account) and the HTTPS fallbacks of clone, pull and push. Loaded
// only when one of them runs (`await import("#lib/ssh.js")`), to keep the startup bundle
// small. The protocol itself is gh's `git_protocol`; Rust probes and fixes.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { goto } from "$app/navigation";
import {
  api,
  errorKind,
  errorMessage,
  withDialog,
  type AuthStatus,
  type CommandError,
  type GitProtocol,
  type Project,
  type SshStatus,
} from "#lib/api.js";
import { announce } from "#lib/announce.svelte.js";
import { gitProtocol } from "#lib/links.js";
import { session } from "#lib/session.svelte.js";
import { t } from "#lib/i18n/index.js";

export const SSH_GUIDE = "https://docs.github.com/en/authentication/connecting-to-github-with-ssh";
export const SSH_KEYS = "https://github.com/settings/keys";

export function protocolName(protocol: GitProtocol): string {
  return t(`login.protocol.${protocol}`);
}

/** Where SSH to GitHub stands, in one sentence (with what to do when there is a way). */
export function statusText(status: SshStatus): string {
  switch (status.state) {
    case "ready":
      return status.login
        ? t("ssh.state.ready", { login: status.login })
        : t("ssh.state.readyAnon");
    case "otherAccount":
      // A deploy key answers as the repository it belongs to.
      return status.login.includes("/")
        ? t("ssh.state.deployKey", { repo: status.login })
        : t("ssh.state.otherAccount", {
            login: status.login,
            active: status.active ?? session.auth?.login ?? "",
          });
    case "noKey":
      return t("ssh.state.noKey");
    case "keyNotOffered":
      return t("ssh.state.keyNotOffered", { key: status.localKey });
    case "hostKeyUnknown":
      return t("ssh.state.hostKeyUnknown");
    case "hostKeyChanged":
      return t("ssh.state.hostKeyChanged");
    case "unreachable":
      return t("ssh.state.unreachable");
    case "notInstalled":
      return t("ssh.state.notInstalled");
    case "failed":
      return t("ssh.state.failed", { error: status.message });
  }
}

/** A probe, a refusal read as "couldn't check". */
export function probe(fresh: boolean): Promise<SshStatus> {
  return api
    .sshStatus(fresh)
    .catch((err): SshStatus => ({ state: "failed", message: errorMessage(err) }));
}

export function hostKeyProblem(status: SshStatus): boolean {
  return status.state === "hostKeyUnknown" || status.state === "hostKeyChanged";
}

/** The problem code of an `ssh` refusal, else null. */
export function sshProblem(err: unknown): string | null {
  return errorKind(err) === "ssh" ? ((err as CommandError).params?.problem ?? null) : null;
}

async function failure(key: string, err: unknown) {
  await withDialog(() =>
    message(t(key, { error: errorMessage(err) }), { title: protocolName("ssh"), kind: "error" }),
  );
}

/** Adds GitHub's published host keys to known_hosts (or replaces the saved one first),
 *  with no question: the click was it. The new status, or null when it failed (said). */
export async function fixHostKey(replace: boolean): Promise<SshStatus | null> {
  try {
    const status = await (replace ? api.replaceGithubHostKey() : api.trustGithubHost());
    announce(t("a11y.sshTrusted"));
    return status;
  } catch (err) {
    await failure("ssh.fixFailed", err);
    return null;
  }
}

/** Settings' Trust GitHub / Replace GitHub's old key: confirmed first. */
export async function confirmHostKey(replace: boolean): Promise<SshStatus | null> {
  const go = await withDialog(() =>
    ask(t(replace ? "ssh.replaceHint" : "ssh.trustHint"), {
      kind: "info",
      okLabel: t(replace ? "ssh.replace" : "ssh.trust"),
      cancelLabel: t("setup.cancel"),
    }),
  );
  return go ? fixHostKey(replace) : null;
}

/** Asks GitHub for the key scope (gh's device flow); the new status, null when not granted. */
export type GrantScope = () => Promise<AuthStatus | null>;

/** Adds this computer's key. GitHub refusing for want of the key scope (the token lost it,
 *  or the app's view of it was stale) asks for it once, then tries once more. The status
 *  after it (said when it isn't ready), or null when it failed (said) or wasn't granted. */
async function setUp(grantScope: GrantScope, retried = false): Promise<SshStatus | null> {
  try {
    const status = await api.setUpSshKey();
    announce(status.state === "ready" ? t("a11y.sshSetUp") : statusText(status));
    return status;
  } catch (err) {
    if (!retried && sshProblem(err) === "needsScope") {
      return (await grantScope()) ? setUp(grantScope, true) : null;
    }
    await failure("ssh.setUpFailed", err);
    return null;
  }
}

/** Set up SSH, confirmed (after `lead`, when given), the key scope granted first when the
 *  token lacks it (gh's device flow; the popover hides meanwhile), then the key added. */
export async function setUpKey(
  grantScope: GrantScope,
  lead?: string,
  cancel = t("setup.cancel"),
): Promise<SshStatus | null> {
  const auth = session.auth;
  const scoped = Boolean(auth?.canAddSshKeys);
  const lines = [t("ssh.setUpConfirm", { login: auth?.login ?? "" })];
  if (lead) lines.unshift(lead);
  if (!scoped) lines.push(t("ssh.needsGrant"));
  const go = await withDialog(() =>
    ask(lines.join("\n\n"), { kind: "info", okLabel: t("ssh.setUp"), cancelLabel: cancel }),
  );
  if (!go) return null;
  // Just granted: GitHub refusing again isn't asked twice.
  if (!scoped && !(await grantScope())) return null;
  return setUp(grantScope, !scoped);
}

/** Where SSH stands when it isn't ready and the app can't fix it from here: said, with
 *  SSH settings as the way on. */
async function report(status: SshStatus) {
  const open = await withDialog(() =>
    ask(statusText(status), {
      title: protocolName("ssh"),
      kind: "warning",
      okLabel: t("ssh.settings"),
      cancelLabel: t("ssh.later"),
    }),
  );
  if (open) await openSettings();
}

/** Settings, scrolled to its SSH row. */
export function openSettings(): Promise<void> {
  return goto("/settings#ssh");
}

/**
 * Once signed in with SSH: checks it again, now as the new account (before the sign-in it
 * could only answer for gh's previous one). Ready is it; no key is set up (`sshKey`: the
 * scope came with the sign-in) or offered; a host key is offered its fix; anything else (a
 * key of another account, one ssh didn't use) is said, never uploaded.
 */
export async function afterSignIn(
  sshKey: boolean,
  run: { step: (step: "checking" | "settingUp" | null) => void; grant: GrantScope },
): Promise<void> {
  run.step("checking");
  const status = await probe(true);
  run.step(null);
  if (status.state === "ready") return;
  let next: SshStatus | null = null;
  if (status.state === "noKey") {
    if (sshKey) {
      run.step("settingUp");
      next = await setUp(run.grant);
    } else {
      next = await setUpKey(run.grant, statusText(status), t("ssh.later"));
    }
  } else if (hostKeyProblem(status)) {
    const replace = status.state === "hostKeyChanged";
    const go = await withDialog(() =>
      ask(`${statusText(status)}\n\n${t(replace ? "ssh.replaceHint" : "ssh.trustHint")}`, {
        title: protocolName("ssh"),
        kind: "warning",
        okLabel: t(replace ? "ssh.replace" : "ssh.trust"),
        cancelLabel: t("ssh.later"),
      }),
    );
    if (go) next = await fixHostKey(replace);
  } else {
    await report(status);
  }
  run.step(null);
  if (next && next.state !== "ready") await report(next);
}

export interface SignInChoice {
  protocol: GitProtocol;
  /** Also ask for `write:public_key`, then add this computer's key once signed in. */
  sshKey: boolean;
}

/** A three-button question: resolves to the label picked, or null (Cancel, Escape). */
async function choose(
  text: string,
  title: string,
  yes: string,
  no: string,
  cancel = t("auth.cancel"),
): Promise<string | null> {
  const answer = await withDialog(() =>
    message(text, { title, kind: "info", buttons: { yes, no, cancel } }),
  );
  return answer === yes || answer === no ? answer : null;
}

/**
 * Before a sign-in: HTTPS or SSH, gh's current protocol first (the default button). SSH is
 * then probed (`onChecking` around it) and anything in the way said, with its fix or HTTPS.
 * Null when cancelled. The probe can only answer for gh's account so far, so `afterSignIn`
 * checks again.
 */
export async function askSignInProtocol(
  onChecking: (checking: boolean) => void,
): Promise<SignInChoice | null> {
  const current = await gitProtocol(true);
  const other: GitProtocol = current === "ssh" ? "https" : "ssh";
  const picked = await choose(
    t("login.protocol.ask"),
    t("login.protocol.title"),
    protocolName(current),
    protocolName(other),
  );
  if (picked === null) return null;
  if ((picked === protocolName(current) ? current : other) === "https") {
    return { protocol: "https", sshKey: false };
  }
  onChecking(true);
  try {
    return await afterProbe(await probe(true), onChecking);
  } finally {
    onChecking(false);
  }
}

async function afterProbe(
  status: SshStatus,
  onChecking: (checking: boolean) => void,
): Promise<SignInChoice | null> {
  const https: SignInChoice = { protocol: "https", sshKey: false };
  const title = protocolName("ssh");
  const useHttps = t("ssh.useHttps");
  if (status.state === "ready") return { protocol: "ssh", sshKey: false };
  onChecking(false);
  if (hostKeyProblem(status)) {
    const replace = status.state === "hostKeyChanged";
    const fix = t(replace ? "ssh.replaceContinue" : "ssh.trustContinue");
    const hint = t(replace ? "ssh.replaceHint" : "ssh.trustHint");
    const answer = await choose(`${statusText(status)}\n\n${hint}`, title, fix, useHttps);
    if (answer === useHttps) return https;
    if (answer === null) return null;
    onChecking(true);
    const next = await fixHostKey(replace);
    return next ? afterProbe(next, onChecking) : null;
  }
  if (status.state === "noKey") {
    const fix = t("ssh.signInSetUp");
    const text = `${statusText(status)}\n\n${t("ssh.setUpHint")}`;
    const answer = await choose(text, title, fix, useHttps);
    if (answer === useHttps) return https;
    return answer === null ? null : { protocol: "ssh", sshKey: true };
  }
  const anyway = t("ssh.useAnyway");
  const answer = await choose(statusText(status), title, anyway, useHttps);
  if (answer === useHttps) return https;
  return answer === null ? null : { protocol: "ssh", sshKey: false };
}

/**
 * Before cloning or switching a clone to SSH: "ssh" when SSH works (or once GitHub's host
 * key is fixed from here), "https" for the fallback (offered only with `https`, a clone's),
 * null to stop (Cancel, or SSH settings opened for a fix that needs more than a click).
 */
export async function checkSsh(name: string, https?: string): Promise<GitProtocol | null> {
  const status = await probe(false);
  if (status.state === "ready") return "ssh";
  const hostKey = hostKeyProblem(status);
  const replace = status.state === "hostKeyChanged";
  const fix = t(hostKey ? (replace ? "ssh.replace" : "ssh.trust") : "ssh.settings");
  let answer: string | null;
  if (https) {
    const text = `${statusText(status)}\n\n${t("clone.sshNotReady")}`;
    answer = await choose(text, name, https, fix, t("clone.cancel"));
    if (answer === https) return "https";
  } else {
    const go = await withDialog(() =>
      ask(statusText(status), {
        title: name,
        kind: "warning",
        okLabel: fix,
        cancelLabel: t("clone.cancel"),
      }),
    );
    answer = go ? fix : null;
  }
  if (answer === null) return null;
  if (!hostKey) {
    await openSettings();
    return null;
  }
  const next = await fixHostKey(replace);
  if (!next) return null;
  return next.state === "ready" ? "ssh" : checkSsh(name, https);
}

/** Before an SSH clone (`checkSsh`, with Clone with HTTPS as the fallback). */
export function checkCloneOverSsh(name: string): Promise<GitProtocol | null> {
  return checkSsh(name, t("clone.withHttps"));
}

/** A clone over SSH failed (kind `ssh`): try again with HTTPS? */
export function offerHttpsClone(text: string): Promise<boolean> {
  return withDialog(() =>
    ask(`${text}\n\n${t("clone.retryHttps")}`, {
      kind: "warning",
      okLabel: t("clone.withHttps"),
      cancelLabel: t("clone.cancel"),
    }),
  );
}

/** Points the clone's origin at GitHub over `protocol` (its `.git/config` only); to SSH only
 *  once SSH works (`checkSsh`). */
export async function setRemote(
  project: Project,
  name: string,
  protocol: GitProtocol,
): Promise<boolean> {
  if (protocol === "ssh" && (await checkSsh(name)) !== "ssh") return false;
  try {
    await api.setCloneRemote(project.id, protocol);
    announce(t("a11y.remoteSet", { name, protocol: protocolName(protocol) }));
    return true;
  } catch (err) {
    await withDialog(() =>
      message(t("ssh.remoteFailed", { error: errorMessage(err) }), { title: name, kind: "error" }),
    );
    return false;
  }
}

/** A pull or push over SSH failed (`err`, said by `text`): offers to switch the clone to
 *  HTTPS, and for GitHub's host key its fix first. True once fixed or switched, so the
 *  caller tries once more; declining writes nothing. */
export async function recoverSsh(project: Project, name: string, text: string, err: unknown) {
  const https = t("clone.useFor", { protocol: protocolName("https") });
  const problem = sshProblem(err);
  if (problem === "hostKeyUnknown" || problem === "hostKeyChanged") {
    const replace = problem === "hostKeyChanged";
    const fix = t(replace ? "ssh.replace" : "ssh.trust");
    const hint = t(replace ? "ssh.replaceHint" : "ssh.trustHint");
    const answer = await choose(
      `${text}\n\n${hint}\n\n${t("ssh.retryHostKey")}`,
      name,
      fix,
      https,
      t("clone.cancel"),
    );
    if (answer === fix) return (await fixHostKey(replace)) !== null;
    return answer === https && setRemote(project, name, "https");
  }
  const yes = await withDialog(() =>
    ask(`${text}\n\n${t("ssh.retryHttps")}`, {
      title: name,
      kind: "warning",
      okLabel: https,
      cancelLabel: t("clone.cancel"),
    }),
  );
  return yes && setRemote(project, name, "https");
}
