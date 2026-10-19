//! Update check — ask GitHub Releases whether a newer Ordnung is published.
//!
//! Engine-shaped per `ordnung-architecture`: pure library, no UI, no policy,
//! no `println!`. The caller (GUI) decides when to run the check, supplies its
//! own running version, and chooses how to surface a hit. The check is a
//! single unauthenticated GitHub API request, well under the 60 req/hr/IP
//! limit at once-per-launch.
//!
//! [`install`] is the in-app update: it fetches the release's DMG, copies the
//! bundle out of it next to the running one and swaps the two. The download
//! is the app's own, so the file carries no quarantine attribute and
//! Gatekeeper never re-assesses it; with a stable signing identity the TCC
//! grants carry over too. Nothing is deleted until the new bundle is in
//! place, and the running process keeps its mapped files until it exits.

use crate::error::{Error, Result};
use crate::tools::DownloadProgress;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// GitHub "latest release" endpoint for the public Ordnung repo. Returns the
/// most recent non-prerelease, non-draft release as JSON.
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Kailazy/Ord/releases/latest";

/// A published release newer than the running build, ready for the GUI to offer
/// as a download. `version` is the tag with any leading `v` stripped (e.g.
/// `0.0.3`); `url` is the human-facing release page to open in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    /// Direct link to the release's universal DMG, when it has one; what
    /// [`install`] fetches.
    pub dmg_url: Option<String>,
}

/// Ask GitHub for the latest release and compare it against `current` (the
/// running build's `CARGO_PKG_VERSION`). Returns:
/// - `Ok(Some(info))` when the published tag parses as strictly newer,
/// - `Ok(None)` when we're up to date (or the tag isn't newer / can't be parsed),
/// - `Err(_)` only on a network/transport failure — the caller treats this as
///   "couldn't check" and stays silent rather than alarming the user.
pub fn check_latest(current: &str) -> Result<Option<UpdateInfo>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let resp = agent
        .get(LATEST_RELEASE_URL)
        // GitHub rejects requests without a User-Agent; identify ourselves.
        .set("User-Agent", "Ordnung-update-check")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| Error::Network(e.to_string()))?;
    let body: serde_json::Value = resp
        .into_json()
        .map_err(|e| Error::Network(e.to_string()))?;
    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let latest = tag.trim_start_matches('v').trim();
    if latest.is_empty() {
        return Ok(None);
    }
    if is_newer(latest, current) {
        // Prefer the release page; fall back to the repo's releases listing.
        let url = body
            .get("html_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("https://github.com/Kailazy/Ord/releases/latest")
            .to_string();
        let dmg_url = body
            .get("assets")
            .and_then(|a| a.as_array())
            .and_then(|assets| {
                assets.iter().find_map(|a| {
                    let name = a.get("name")?.as_str()?;
                    (name.starts_with("Ord-") && name.ends_with("-macos-universal.dmg"))
                        .then(|| a.get("browser_download_url")?.as_str().map(str::to_string))
                        .flatten()
                })
            });
        Ok(Some(UpdateInfo {
            version: latest.to_string(),
            url,
            dmg_url,
        }))
    } else {
        Ok(None)
    }
}

/// The `.app` bundle the running process lives in, or `None` when it runs
/// bare (a `cargo run` binary has nothing to swap).
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // <bundle>.app/Contents/MacOS/<exe>
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension().is_some_and(|e| e == "app") && bundle.join("Contents/Info.plist").is_file())
        .then(|| bundle.to_path_buf())
}

/// Fetch `info`'s DMG and put its bundle in place of the running one.
/// Returns the bundle's path (the caller relaunches it, see [`relaunch`]),
/// or `None` when `cancel` was set during the download. The old bundle is
/// moved aside and removed only after the new one is in place; a failure
/// anywhere before that leaves the installed app untouched.
pub fn install(
    info: &UpdateInfo,
    progress: &mut dyn FnMut(DownloadProgress),
    cancel: &AtomicBool,
) -> Result<Option<PathBuf>> {
    let bundle = running_bundle()
        .ok_or_else(|| Error::Invalid("not running from an app bundle".into()))?;
    let dmg_url = info
        .dmg_url
        .as_deref()
        .ok_or_else(|| Error::Invalid("this release has no DMG to install".into()))?;
    let parent = bundle.parent().unwrap_or_else(|| Path::new("/Applications"));
    let pid = std::process::id();
    let tmp = std::env::temp_dir().join(format!("ord-update-{pid}"));
    let io = |path: &Path, source: std::io::Error| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    std::fs::create_dir_all(&tmp).map_err(|e| io(&tmp, e))?;
    let result = (|| {
        let dmg = tmp.join("Ord.dmg");
        if !crate::tools::download(dmg_url, "the update", &dmg, progress, cancel)? {
            return Ok(None);
        }
        let mnt = tmp.join("mnt");
        run(
            "hdiutil",
            &["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"],
            &[&mnt, &dmg],
        )?;
        let copied = (|| {
            let app = std::fs::read_dir(&mnt)
                .map_err(|e| io(&mnt, e))?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or_else(|| Error::Invalid("the DMG holds no app".into()))?;
            // Copied next to the installed bundle (same volume), so the swap
            // below is two renames.
            let staged = parent.join(format!(".Ord-update-{pid}.app"));
            let _ = std::fs::remove_dir_all(&staged);
            run("ditto", &[], &[&app, &staged])?;
            Ok::<PathBuf, Error>(staged)
        })();
        let _ = run("hdiutil", &["detach", "-force"], &[&mnt]);
        let staged = copied?;
        let plist = std::fs::read_to_string(staged.join("Contents/Info.plist")).unwrap_or_default();
        if !plist.contains(&format!("<string>{}</string>", info.version)) {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(Error::Invalid(format!(
                "the downloaded app is not version {}",
                info.version
            )));
        }
        let old = parent.join(format!(".Ord-old-{pid}.app"));
        std::fs::rename(&bundle, &old).map_err(|e| io(&bundle, e))?;
        if let Err(e) = std::fs::rename(&staged, &bundle) {
            // Put the old one back; nothing is lost.
            let _ = std::fs::rename(&old, &bundle);
            return Err(io(&bundle, e));
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(Some(bundle.clone()))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Open `bundle` once this process has exited: a detached shell waits for
/// the pid to go away, then `open`s the app. Call it, then quit.
pub fn relaunch(bundle: &Path) {
    let pid = std::process::id();
    let script = format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; open \"{}\"",
        bundle.display()
    );
    let _ = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Run a system tool, turning a non-zero exit into an error naming it.
fn run(tool: &str, flags: &[&str], paths: &[&Path]) -> Result<()> {
    let out = Command::new(tool)
        .args(flags)
        .args(paths)
        .output()
        .map_err(|e| Error::Invalid(format!("{tool}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{tool} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// True when `latest` is a strictly greater version than `current`. Both are
/// dotted numeric strings (`0.0.3`); a pre-release suffix (`0.1.0-rc1`) is
/// ignored — we compare on the numeric release components only. An unparseable
/// component counts as 0, and a string that yields no numbers never wins, so a
/// malformed tag can't spuriously trigger an "update available" prompt.
fn is_newer(latest: &str, current: &str) -> bool {
    let a = parse_version(latest);
    let b = parse_version(current);
    a > b
}

/// Parse a dotted version into comparable numeric components, dropping any
/// `-suffix`/`+build` and treating non-numeric parts as 0. `0.0.3` → `[0,0,3]`.
fn parse_version(v: &str) -> Vec<u64> {
    let core = v
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    core.split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_minor_major() {
        assert!(is_newer("0.0.3", "0.0.2"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn equal_or_older_is_not_newer() {
        assert!(!is_newer("0.0.2", "0.0.2"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        assert!(!is_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn shorter_versions_compare_by_components() {
        // [1] vs [1,0,0]: Vec ordering makes the longer one greater, so a bare
        // "1" is *not* newer than "1.0.0" — and "1.0.1" is newer than "1".
        assert!(!is_newer("1", "1.0.0"));
        assert!(is_newer("1.0.1", "1"));
    }

    #[test]
    fn prerelease_suffix_ignored() {
        // The numeric core wins; the -rc suffix is dropped on both sides.
        assert!(is_newer("0.1.0-rc1", "0.0.9"));
        assert!(!is_newer("0.1.0-rc1", "0.1.0"));
    }

    #[test]
    fn garbage_never_wins() {
        assert!(!is_newer("", "0.0.1"));
        assert!(!is_newer("not-a-version", "0.0.1"));
    }

    #[test]
    fn parse_strips_v_prefix() {
        assert_eq!(parse_version("v0.0.2"), vec![0, 0, 2]);
        assert_eq!(parse_version("0.0.2"), vec![0, 0, 2]);
    }
}
