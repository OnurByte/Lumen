//! Session question lists: the questions an operator enters for a broadcast, stored per pack in
//! `$XDG_CONFIG_HOME/lumen/sessions/<pack>.toml` and pushed live to the runtime.
use gpui::Global;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_QUESTIONS: usize = 64;
pub const MAX_QUESTION_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    pub questions: Vec<String>,
    pub minutes: u16,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            questions: Vec::new(),
            minutes: 10,
        }
    }
}

/// Shared with every page: the active pack's session plus live OBS status for the dashboard.
#[derive(Debug, Clone, Default)]
pub struct Broadcast {
    pub session: Session,
    pub obs: lumen_obs::Status,
}

impl Global for Broadcast {}

fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("lumen/sessions"))
}

fn pack_stem(pack: &Path) -> String {
    pack.file_stem()
        .map_or_else(|| "pack".into(), |s| s.to_string_lossy().into_owned())
}

/// Opening topic + rotation from the pack file, used when no session was saved yet.
fn from_pack(pack: &Path) -> Session {
    let value: toml::Value = std::fs::read_to_string(pack)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or(toml::Value::Table(Default::default()));
    let mut questions: Vec<String> = value
        .get("topic")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .into_iter()
        .collect();
    for topic in value
        .get("topics")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
    {
        if !questions.iter().any(|q| q == topic) {
            questions.push(topic.to_owned());
        }
    }
    Session {
        questions,
        minutes: value
            .get("topic_minutes")
            .and_then(|v| v.as_integer())
            .map_or(10, |m| m.clamp(1, 180) as u16),
    }
}

impl Session {
    pub fn load(pack: &Path) -> Self {
        dir()
            .map(|d| d.join(format!("{}.toml", pack_stem(pack))))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str::<Session>(&t).ok())
            .map(Session::sanitized)
            .filter(|s| !s.questions.is_empty())
            .unwrap_or_else(|| from_pack(pack).sanitized())
    }

    pub fn save(&self, pack: &Path) -> Result<(), String> {
        let dir = dir().ok_or("no config directory")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.toml", pack_stem(pack)));
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(
            &tmp,
            toml::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn sanitized(mut self) -> Self {
        self.questions = self
            .questions
            .into_iter()
            .map(|q| clean(&q))
            .filter(|q| !q.is_empty())
            .take(MAX_QUESTIONS)
            .collect();
        self.minutes = self.minutes.clamp(1, 180);
        self
    }

    pub fn total_seconds(&self) -> u32 {
        self.questions.len() as u32 * u32::from(self.minutes) * 60
    }

    pub fn topic_list(&self) -> Option<lumen_protocol::TopicList> {
        let list = lumen_protocol::TopicList {
            topics: self.questions.clone(),
            minutes: self.minutes,
        };
        list.valid().then_some(list)
    }
}

/// One line, no control characters, bounded length.
pub fn clean(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_QUESTION_CHARS)
        .collect()
}

/// "1 h 30 min" / "40 min".
pub fn duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_duration_is_questions_times_minutes() {
        let s = Session {
            questions: vec!["a".into(); 9],
            minutes: 10,
        };
        assert_eq!(s.total_seconds(), 5400);
        assert_eq!(duration(5400), "1 h 30 min");
        assert_eq!(duration(2400), "40 min");
        assert_eq!(duration(7200), "2 h");
    }

    #[test]
    fn questions_are_cleaned_and_bounded() {
        let s = Session {
            questions: vec!["  Laiklik\n dine\tkarşı mı? ".into(), "   ".into()],
            minutes: 0,
        }
        .sanitized();
        assert_eq!(s.questions, vec!["Laiklik dine karşı mı?".to_owned()]);
        assert_eq!(s.minutes, 1);
        assert!(clean(&"x".repeat(1000)).chars().count() <= MAX_QUESTION_CHARS);
    }

    #[test]
    fn pack_rotation_seeds_a_new_session() {
        let dir = tempfile_dir();
        let pack = dir.join("p.toml");
        std::fs::write(
            &pack,
            "topic = \"A?\"\ntopics = [\"B?\", \"A?\"]\ntopic_minutes = 7\n",
        )
        .unwrap();
        let s = from_pack(&pack);
        assert_eq!(
            (s.questions.clone(), s.minutes),
            (vec!["A?".to_owned(), "B?".to_owned()], 7)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-session-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
