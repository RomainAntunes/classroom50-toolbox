//! The OS release as people name it, read from files or the registry rather than by running
//! a program. Pure parsers, compiled and tested on every OS; each platform file only reads.
// Each OS uses one parser; the others are only exercised by the tests.
#![cfg_attr(not(test), allow(dead_code))]

/// `ProductVersion` from macOS's `SystemVersion.plist` (an XML plist).
pub fn macos(plist: &str) -> Option<String> {
    let after_key = plist.split("<key>ProductVersion</key>").nth(1)?;
    let value = after_key.trim_start().strip_prefix("<string>")?;
    let version = value.split("</string>").next()?.trim();
    (!version.is_empty()).then(|| format!("macOS {version}"))
}

/// `PRETTY_NAME` from `/etc/os-release`, else `NAME` and `VERSION_ID`.
pub fn linux(os_release: &str) -> Option<String> {
    let field = |name: &str| {
        os_release.lines().find_map(|line| {
            let value = line.trim().strip_prefix(name)?.strip_prefix('=')?;
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
    };
    field("PRETTY_NAME").or_else(|| match (field("NAME"), field("VERSION_ID")) {
        (Some(name), Some(version)) => Some(format!("{name} {version}")),
        (name, _) => name,
    })
}

/// Windows 11 still says "Windows 10" in `ProductName`; its builds start at 22000.
pub fn windows(display_version: Option<&str>, build: Option<&str>) -> String {
    let number = build.and_then(|b| b.trim().parse::<u32>().ok());
    let name = match number {
        Some(n) if n >= 22000 => "Windows 11",
        Some(_) => "Windows 10",
        None => "Windows",
    };
    let mut out = name.to_owned();
    if let Some(version) = display_version.map(str::trim).filter(|v| !v.is_empty()) {
        out.push(' ');
        out.push_str(version);
    }
    if let Some(n) = number {
        out.push_str(&format!(" (build {n})"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_macos_product_version() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>ProductBuildVersion</key>
	<string>25F71</string>
	<key>ProductVersion</key>
	<string>26.5</string>
</dict>
</plist>"#;
        assert_eq!(macos(plist).as_deref(), Some("macOS 26.5"));
        assert_eq!(macos("<dict></dict>"), None);
        assert_eq!(macos("<key>ProductVersion</key><string></string>"), None);
    }

    #[test]
    fn reads_the_linux_pretty_name() {
        let release = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(linux(release).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(
            linux("NAME=Fedora\nVERSION_ID=41\n").as_deref(),
            Some("Fedora 41")
        );
        assert_eq!(linux("NAME='Arch Linux'\n").as_deref(), Some("Arch Linux"));
        assert_eq!(linux("PRETTY_NAME=\"\"\n"), None);
        assert_eq!(linux(""), None);
    }

    #[test]
    fn names_windows_from_its_build() {
        assert_eq!(
            windows(Some("24H2"), Some("26100")),
            "Windows 11 24H2 (build 26100)"
        );
        assert_eq!(
            windows(Some("22H2"), Some("19045")),
            "Windows 10 22H2 (build 19045)"
        );
        assert_eq!(windows(None, None), "Windows");
        assert_eq!(windows(Some(" "), Some("x")), "Windows");
    }
}
