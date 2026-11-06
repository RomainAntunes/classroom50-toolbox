// HTTPS or SSH with GitHub: the sign-in question, SSH's status in words, its fixes (GitHub's
// host keys, a key on the account) and the HTTPS fallbacks of clone, pull and push. Loaded
// only when one of them runs (`await import("#lib/ssh.js")`), to keep the startup bundle
// small. The protocol itself is gh's `git_protocol`; Rust probes and fixes.
import { ask, message } from "@tauri-apps/plugin-dialog";
import { goto } from "$app/navigation";
import {
  api,
  errorMessage,
  withDialog,
  type GitProtocol,
  type Project,
  type SshStatus,
} from "#lib/api.js";
import { announce } from "#lib/announce.svelte.js";
import { gitProtocol } from "#lib/links.js";
import { grant } from "#lib/login.svelte.js";
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

/** Adds GitHub's published host keys to known_hosts (or replaces the saved one first),
 *  with no question: the click was it. The new status, or null when it failed (said). */
export async function fixHostKey(replace: boolean): Promise<SshStatus | null> {
  try {
    const status = await (replace ? api.replaceGithubHostKey() : api.trustGithubHost());
    announce(t("a11y.sshTrusted"));
    return status;
  } catch (err) {
    await withDialog(() =>
      message(t("ssh.fixFailed", { error: errorMessage(err) }), { kind: "error" }),
    );
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

async function setUp(): Promise<SshStatus | null> {
  try {
    const status = await api.setUpSshKey();
    announce(t("a11y.sshSetUp"));
    return status;
  } catch (err) {
    await withDialog(() =>
      message(t("ssh.setUpFailed", { error: errorMessage(err) }), { kind: "error" }),
    );
    return null;
  }
}

/** Settings' Set up SSH: confirmed, then the key scope granted first when the token lacks
 *  it (gh's device flow; the popover hides meanwhile), then the key added. */
export async function setUpKey(): Promise<SshStatus | null> {
  const auth = session.auth;
  const scoped = Boolean(auth?.canAddSshKeys);
  const lines = [t("ssh.setUpConfirm", { login: auth?.login ?? "" })];
  if (!scoped) lines.push(t("ssh.needsGrant"));
  const go = await withDialog(() =>
    ask(lines.join("\n\n"), {
      kind: "info",
      okLabel: t("ssh.setUp"),
      cancelLabel: t("setup.cancel"),
    }),
  );
  if (!go) return null;
  if (!scoped && !(await grant(["writePublicKey"]))?.canAddSshKeys) return null;
  return setUp();
}

/** After a sign-in that asked for the key scope: adds the key. */
export async function setUpAfterSignIn(): Promise<void> {
  await setUp();
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
 * Null when cancelled.
 */
export async function askSignInProtocol(
  onChecking: (checking: boolean) => void,
): Promise<SignInChoice | null> {
  const current = await gitProtocol();
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
 * Before an SSH clone: "ssh" when SSH works (or once GitHub's host key is fixed from here),
 * "https" for the fallback, null to stop (Cancel, or SSH settings opened for a fix that
 * needs more than a click).
 */
export async function checkCloneOverSsh(name: string): Promise<GitProtocol | null> {
  const status = await probe(false);
  if (status.state === "ready") return "ssh";
  const https = t("clone.withHttps");
  const hostKey = hostKeyProblem(status);
  const replace = status.state === "hostKeyChanged";
  const fix = t(hostKey ? (replace ? "ssh.replace" : "ssh.trust") : "ssh.settings");
  const text = `${statusText(status)}\n\n${t("clone.sshNotReady")}`;
  const answer = await choose(text, name, https, fix, t("clone.cancel"));
  if (answer === https) return "https";
  if (answer === null) return null;
  if (!hostKey) {
    await goto("/settings");
    return null;
  }
  const next = await fixHostKey(replace);
  if (!next) return null;
  return next.state === "ready" ? "ssh" : checkCloneOverSsh(name);
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

/** Points the clone's origin at GitHub over `protocol` (its `.git/config` only). */
export async function setRemote(
  project: Project,
  name: string,
  protocol: GitProtocol,
): Promise<boolean> {
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

/** A pull or push over SSH failed (`text` says why): offers to switch the clone to HTTPS.
 *  True once switched, so the caller tries once more; declining writes nothing. */
export async function switchToHttps(project: Project, name: string, text: string) {
  const yes = await withDialog(() =>
    ask(`${text}\n\n${t("ssh.retryHttps")}`, {
      title: name,
      kind: "warning",
      okLabel: t("clone.useFor", { protocol: protocolName("https") }),
      cancelLabel: t("accept.cancel"),
    }),
  );
  return yes && setRemote(project, name, "https");
}
