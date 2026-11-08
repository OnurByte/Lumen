//! Updates from GitHub releases: find the latest one, install it with that release's own
//! `install.sh` (which verifies SHA-256), and roll back to the binaries it kept.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

const REPOSITORY: &str = "OnurByte/Lumen";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PREVIOUS_DIR: &str = ".lumen-previous";
const BINARIES: [&str; 4] = ["lumend", "lumen-studio", "lumen-live", "lumen"];
const INSTALLER: &str = if cfg!(windows) { "install.ps1" } else { "install.sh" };

/// `v1.2.3` or `1.2.3`; a pre-release suffix is ignored.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("Lumen/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// Summarises GitHub's "latest release" JSON against this build.
pub fn summarize(release: &Value) -> Result<Value, String> {
    let tag = release["tag_name"].as_str().filter(|t| parse_version(t).is_some()).ok_or("The latest release has no valid version.")?;
    let url = release["html_url"].as_str().unwrap_or_default();
    if !url.is_empty() && !url.starts_with(&format!("https://github.com/{REPOSITORY}/")) {
        return Err("The release has an unexpected address.".into());
    }
    let notes: String = release["body"].as_str().unwrap_or_default().chars().take(1200).collect();
    Ok(json!({
        "current": VERSION,
        "tag": tag,
        "url": url,
        "notes": notes,
        "published": release["published_at"].as_str().unwrap_or_default().chars().take(10).collect::<String>(),
        "available": parse_version(tag) > parse_version(VERSION),
    }))
}

pub async fn check() -> Result<Value, String> {
    let response = client()?
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "Could not reach GitHub to check for updates.".to_string())?;
    match response.status().as_u16() {
        200 => summarize(&response.json().await.map_err(|e| e.to_string())?),
        404 => Err("No release has been published yet.".into()),
        code => Err(format!("GitHub answered {code} while checking for updates.")),
    }
}

fn binary_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().and_then(|e| e.canonicalize()).map_err(|e| e.to_string())?;
    exe.parent().map(PathBuf::from).ok_or_else(|| "The program has no folder.".to_string())
}

/// Installs the latest release into the folder this program runs from.
pub async fn install() -> Result<String, String> {
    let status = check().await?;
    let tag = status["tag"].as_str().unwrap_or_default().to_owned();
    if status["available"] != true {
        return Ok(format!("You already have the latest version ({VERSION})."));
    }
    let url = format!("https://raw.githubusercontent.com/{REPOSITORY}/{tag}/{INSTALLER}");
    let response = client()?.get(&url).send().await.map_err(|_| "Could not download the installer.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("GitHub answered {} for the installer.", response.status()));
    }
    let script = response.bytes().await.map_err(|e| e.to_string())?;
    if script.len() > 256 * 1024 {
        return Err("The installer is unexpectedly large.".into());
    }
    let path = std::env::temp_dir().join(format!("lumen-install-{}-{INSTALLER}", std::process::id()));
    tokio::fs::write(&path, &script).await.map_err(|e| e.to_string())?;
    let mut command = if cfg!(windows) {
        let mut shell = tokio::process::Command::new("powershell");
        shell.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        shell
    } else {
        tokio::process::Command::new("sh")
    };
    let result = command.arg(&path).env("LUMEN_VERSION", &tag).env("LUMEN_BIN_DIR", binary_dir()?).env("LUMEN_NO_DEPS", "1").status().await;
    let _ = tokio::fs::remove_file(&path).await;
    match result {
        Ok(s) if s.success() => Ok(format!("Updated to Lumen {tag}. Restart Lumen to use it.")),
        Ok(s) => Err(format!("The installer failed ({s}).")),
        Err(e) => Err(format!("The installer could not start: {e}")),
    }
}

pub fn rollback() -> Result<String, String> {
    let dir = binary_dir()?;
    let previous = dir.join(PREVIOUS_DIR);
    let mut restored = 0;
    for name in BINARIES {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let kept = previous.join(&file);
        if kept.is_file() {
            // Copy beside the target, then rename: a running copy keeps working.
            let staged = dir.join(format!(".{file}.rollback"));
            std::fs::copy(&kept, &staged).map_err(|e| e.to_string())?;
            let target = dir.join(&file);
            if cfg!(windows) && target.exists() {
                // A running .exe cannot be replaced, but it can be renamed aside.
                let aside = dir.join(format!(".{file}.old"));
                let _ = std::fs::remove_file(&aside);
                std::fs::rename(&target, aside).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&staged, target).map_err(|e| e.to_string())?;
            restored += 1;
        }
    }
    if restored == 0 { Err("No previous version is kept, so there is nothing to roll back.".into()) } else { Ok("Restored the previous version. Restart Lumen to use it.".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn release_is_summarized_and_checked() {
        let s = summarize(&json!({"tag_name": "v99.0.0", "html_url": "https://github.com/OnurByte/Lumen/releases/tag/v99.0.0", "body": "Notes", "published_at": "2026-11-08T10:00:00Z"})).unwrap();
        assert_eq!((s["available"].as_bool(), s["published"].as_str()), (Some(true), Some("2026-11-08")));
        assert!(summarize(&json!({"tag_name": "v1.0.0", "html_url": "https://evil.example/x"})).is_err());
        assert!(summarize(&json!({"tag_name": "nightly"})).is_err());
    }
}
