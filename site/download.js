// Points the Download button at the visitor's installer in the latest published release, and
// lists the others. Without JavaScript, or when GitHub can't be reached, the button keeps
// linking to the release page.
const REPO = "RomainAntunes/classroom50-toolbox";

// Installers only: updater archives, signatures and latest.json are left out. The order is
// the order of the "Other platforms" list.
const KINDS = [
  { test: /_aarch64\.dmg$/, os: "mac-arm", label: "macOS (Apple Silicon)" },
  { test: /_x64\.dmg$/, os: "mac-intel", label: "macOS (Intel)" },
  { test: /_x64-setup\.exe$/, os: "windows", label: "Windows" },
  { test: /_x64_[^_]+\.msi$/, os: "windows-msi", label: "Windows (MSI)" },
  { test: /_arm64-setup\.exe$/, os: "windows-arm", label: "Windows on Arm" },
  { test: /_arm64_[^_]+\.msi$/, os: "windows-arm-msi", label: "Windows on Arm (MSI)" },
  { test: /_amd64\.AppImage$/, os: "linux", label: "Linux (AppImage)" },
  { test: /_amd64\.deb$/, os: "linux-deb", label: "Debian, Ubuntu (.deb)" },
  { test: /\.x86_64\.rpm$/, os: "linux-rpm", label: "Fedora, RHEL (.rpm)" },
  { test: /_aarch64\.AppImage$/, os: "linux-arm", label: "Linux on Arm (AppImage)" },
  { test: /_arm64\.deb$/, os: "linux-arm-deb", label: "Debian, Ubuntu on Arm (.deb)" },
  { test: /\.aarch64\.rpm$/, os: "linux-arm-rpm", label: "Fedora, RHEL on Arm (.rpm)" },
];

// Whether the browser runs on a 64-bit Arm CPU, when it says so. Chromium tells the
// architecture through client hints; Firefox on Linux keeps it in the user agent. Others
// (Safari, Firefox on Windows, which reports x64 even on Arm) can't tell.
async function isArm() {
  if (/aarch64|arm64/i.test(navigator.userAgent)) return true;
  try {
    const { architecture, bitness } = await navigator.userAgentData.getHighEntropyValues([
      "architecture",
      "bitness",
    ]);
    return architecture === "arm" && bitness !== "32";
  } catch {
    // Not Chromium.
  }
  return false;
}

async function detectOs() {
  const ua = navigator.userAgent;
  // Windows and Linux default to x64 when the browser can't tell: Windows on Arm runs the x64
  // build too (emulated), and most Linux desktops are x64.
  if (/Windows/.test(ua)) return (await isArm()) ? "windows-arm" : "windows";
  if (/Linux/.test(ua) && !/Android/.test(ua)) return (await isArm()) ? "linux-arm" : "linux";
  if (!/Mac OS X/.test(ua) || /iPhone|iPad/.test(ua)) return null;
  // Safari and Firefox report every Mac as Intel; only Chromium tells the architecture. Most
  // Macs sold since 2020 are Apple Silicon, so that's the default.
  try {
    const { architecture } = await navigator.userAgentData.getHighEntropyValues(["architecture"]);
    if (architecture === "x86") return "mac-intel";
  } catch {
    // Not Chromium.
  }
  return "mac-arm";
}

async function main() {
  const primary = document.getElementById("primary");
  const version = document.getElementById("version");
  const response = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`);
  if (!response.ok) return;
  const release = await response.json();

  const files = KINDS.flatMap((kind) => {
    const asset = release.assets.find((a) => kind.test.test(a.name));
    return asset ? [{ ...kind, url: asset.browser_download_url }] : [];
  });
  const os = await detectOs();
  // A release from before the Arm builds: Windows on Arm runs the x64 build emulated, while
  // Linux on Arm has nothing to run, so the button keeps pointing at the release page.
  const mine =
    files.find((f) => f.os === os) ??
    (os === "windows-arm" ? files.find((f) => f.os === "windows") : undefined);
  const date = new Date(release.published_at).toLocaleDateString(document.documentElement.lang, {
    dateStyle: "long",
  });

  if (mine) {
    primary.href = mine.url;
    primary.textContent = `Download for ${mine.label}`;
  }
  version.textContent = `Version ${release.tag_name.replace(/^v/, "")}, ${date}. Free and open source.`;

  const list = document.getElementById("files");
  for (const file of files.filter((f) => f !== mine)) {
    const item = document.createElement("li");
    const link = document.createElement("a");
    link.href = file.url;
    link.textContent = file.label;
    item.append(link);
    list.append(item);
  }
  document.getElementById("others").hidden = list.children.length === 0;
}

main().catch(() => {});

// The menu bar's clock, in the visitor's own time, like the one beside the real icon.
function tick() {
  const clock = document.getElementById("clock");
  const now = new Date();
  const day = now.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" });
  const time = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  clock.textContent = `${day.replace(",", "")}  ${time}`;
}

tick();
setInterval(tick, 15_000);
