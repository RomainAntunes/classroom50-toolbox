// Points the Download button at the visitor's installer in the latest published release, and
// lists the others. Without JavaScript, or when GitHub can't be reached, the button keeps
// linking to the release page.
const REPO = "RomainAntunes/classroom50-toolbox";

// Installers only: updater archives, signatures and latest.json are left out.
const KINDS = [
  { test: /_aarch64\.dmg$/, os: "mac-arm", label: "macOS (Apple Silicon)" },
  { test: /_x64\.dmg$/, os: "mac-intel", label: "macOS (Intel)" },
  { test: /-setup\.exe$/, os: "windows", label: "Windows" },
  { test: /\.msi$/, os: "windows-msi", label: "Windows (MSI)" },
  { test: /\.AppImage$/, os: "linux", label: "Linux (AppImage)" },
  { test: /\.deb$/, os: "linux-deb", label: "Debian, Ubuntu (.deb)" },
  { test: /\.rpm$/, os: "linux-rpm", label: "Fedora, RHEL (.rpm)" },
];

async function detectOs() {
  const ua = navigator.userAgent;
  if (/Windows/.test(ua)) return "windows";
  if (/Linux/.test(ua) && !/Android/.test(ua)) return "linux";
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
  const mine = files.find((f) => f.os === os);
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
