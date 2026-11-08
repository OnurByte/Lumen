//! First-run state, shows the user names themselves, and updates from GitHub releases.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY_URL: &str = "https://github.com/OnurByte/Lumen";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Prefs {
    onboarded: bool,
}

fn prefs_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("lumen/app.toml"))
}

pub fn onboarded() -> bool {
    prefs_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str::<Prefs>(&t).ok()).is_some_and(|p| p.onboarded)
}

pub fn set_onboarded() -> Result<(), String> {
    let path = prefs_path().ok_or("Could not find your settings folder.")?;
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let text = toml::to_string(&Prefs { onboarded: true }).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// `lumen-live` beside this binary, else from PATH. It owns the network and installer code.
pub fn lumen_live() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join(format!("lumen-live{}", std::env::consts::EXE_SUFFIX)))).filter(|p| p.is_file()).unwrap_or_else(|| "lumen-live".into())
}

/// The display name of a show file; its file name if it has none.
pub fn show_title(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|v| v.get("name")?.as_str().map(str::to_owned))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()))
}

pub fn rename_show(path: &Path, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for your show.".into());
    }
    let mut value: toml::Value = toml::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    value.as_table_mut().ok_or("This show file is damaged.")?.insert("name".into(), name.into());
    std::fs::write(path, toml::to_string(&value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn slug(name: &str) -> String {
    let raw: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let joined = raw.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if joined.is_empty() { "my-show".into() } else { joined }
}

/// Writes a new two-host show into `personas` without touching an existing one.
pub fn create_show(personas: &Path, name: &str, question: &str, hosts: [&str; 2]) -> Result<PathBuf, String> {
    let agent = |host: &str, stance: &str, side: &str, voice: &str, temperature: f64| {
        toml::toml! {
            name = host
            persona = (format!("You are {host}, a quick, warm and plain-spoken TV debater with a sharp sense of humour."))
            position = stance
            side = side
            voice = voice
            provider = "codex"
            model = "gpt-5.6-luna"
            temperature = temperature
            max_tokens = 220
        }
    };
    let pack = toml::toml! {
        name = name
        topic = question
        topic_minutes = 10
        format = "free_debate"
        agents = [
            (agent(hosts[0], "Argue that the answer to each question is yes. Defend that side in every turn.", "Yes", "female_2", 0.75)),
            (agent(hosts[1], "Argue that the answer to each question is no. Defend that side in every turn.", "No", "male_1", 0.8))
        ]
    };
    std::fs::create_dir_all(personas).map_err(|e| e.to_string())?;
    let base = slug(name);
    let path = (0..1000)
        .map(|i| personas.join(if i == 0 { format!("{base}.toml") } else { format!("{base}-{}.toml", i + 1) }))
        .find(|p| !p.exists())
        .ok_or("Too many shows with this name.")?;
    std::fs::write(&path, toml::to_string(&pack).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Finder match: every word of the query appears in the text.
pub fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| text.contains(word))
}

/// `lumen-live update --check` output.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub current: String,
    pub tag: String,
    pub url: String,
    pub notes: String,
    pub published: String,
    pub available: bool,
}

fn last_error(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).lines().last().unwrap_or("Something went wrong.").to_owned()
}

pub fn check_update() -> Result<Release, String> {
    let out = std::process::Command::new(lumen_live()).args(["update", "--check"]).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(last_error(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn install_update() -> Result<(), String> {
    let out = std::process::Command::new(lumen_live()).arg("update").stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(last_error(&out.stderr)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_show_is_named_by_the_user_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("lumen-product-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = create_show(&dir, "Sunday Night: Debate!", "Is cereal soup?", ["Ada", "Max"]).unwrap();
        assert_eq!(first.file_name().unwrap(), "sunday-night-debate.toml");
        let second = create_show(&dir, "Sunday Night: Debate!", "Other?", ["A", "B"]).unwrap();
        assert_eq!(second.file_name().unwrap(), "sunday-night-debate-2.toml");
        assert_eq!(show_title(&first), "Sunday Night: Debate!");
        rename_show(&first, "  New name ").unwrap();
        assert_eq!(show_title(&first), "New name");
        assert!(rename_show(&first, "   ").is_err());
        let pack: toml::Value = toml::from_str(&std::fs::read_to_string(&first).unwrap()).unwrap();
        assert_eq!(pack["agents"].as_array().unwrap().len(), 2);
        assert_eq!(pack["topic"].as_str(), Some("Is cereal soup?"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finder_matches_every_word() {
        assert!(matches("add con", "Add a connection"));
        assert!(matches("", "anything"));
        assert!(!matches("add x", "Add a connection"));
    }
}
