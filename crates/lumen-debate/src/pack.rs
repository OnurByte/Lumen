//! Operator-owned show configuration. Keys are referenced by environment-variable name.
use crate::{director::Format, provider::OpenAiCompatible};
use serde::{Deserialize, Serialize};

/// Where an agent's lines come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// OpenAI-compatible streaming chat endpoint (local model or cloud API key).
    #[default]
    Http,
    /// The locally signed-in `codex` CLI (ChatGPT account); `model` names the GPT model.
    Codex,
}

impl Provider {
    fn is_http(&self) -> bool {
        *self == Provider::Http
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub name: String,
    pub persona: String,
    pub position: String,
    /// Short on-screen side label viewers read at a glance, e.g. "Liderler belirler".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Stage avatar preset (e.g. "fesli", "ogretmen"); empty uses the default look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub voice: String,
    #[serde(default, skip_serializing_if = "Provider::is_http")]
    pub provider: Provider,
    pub model: String,
    /// Required for `http`; unused for `codex`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    pub temperature: f32,
    pub max_tokens: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShowPack {
    pub name: String,
    /// Opening topic; also the first entry of the rotation.
    pub topic: String,
    /// Further topics, rotated in order after `topic` every `topic_minutes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default = "default_topic_minutes")]
    pub topic_minutes: u16,
    /// Shared skill sheets (`packs/skills/_shared/<name>.md`) given to both agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    pub format: Format,
    pub agents: [AgentConfig; 2],
}

fn default_topic_minutes() -> u16 {
    10
}

impl ShowPack {
    /// Full rotation: `topic` followed by `topics`, without duplicates.
    pub fn rotation(&self) -> Vec<String> {
        let mut all = vec![self.topic.clone()];
        for topic in &self.topics {
            if !all.contains(topic) {
                all.push(topic.clone());
            }
        }
        all
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > 65536 {
            return Err("show pack too large".into());
        }
        let pack: Self = toml::from_str(source).map_err(|_| "invalid show pack TOML")?;
        pack.check()?;
        Ok(pack)
    }

    /// Validates bounds and endpoint policy without reparsing.
    pub fn check(&self) -> Result<(), String> {
        let pack = self;
        let bounded = |s: &str, limit| !s.trim().is_empty() && s.len() <= limit;
        if !bounded(&pack.name, 128)
            || !bounded(&pack.topic, 1000)
            || pack.agents[0].name == pack.agents[1].name
            || pack.topics.len() >= crate::director::MAX_TOPICS
            || pack.topics.iter().any(|topic| !bounded(topic, 1000))
            || !(1..=180).contains(&pack.topic_minutes)
            || pack.skills.len() > 8
            || pack.skills.iter().any(|name| {
                name.is_empty()
                    || name.len() > 64
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err("invalid show identity or topic".into());
        }
        for agent in &pack.agents {
            if !bounded(&agent.name, 128)
                || !bounded(&agent.persona, 8192)
                || !bounded(&agent.position, 4096)
                || !bounded(&agent.voice, 128)
                || agent
                    .side
                    .as_ref()
                    .is_some_and(|side| !bounded(side, 40) || side.chars().count() > 28)
                || !agent.temperature.is_finite()
                || !(0.0..=2.0).contains(&agent.temperature)
                || !(1..=1024).contains(&agent.max_tokens)
                || agent.api_key_env.as_ref().is_some_and(|name| {
                    name.is_empty()
                        || name.len() > 128
                        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        || name.as_bytes()[0].is_ascii_digit()
                })
            {
                return Err("invalid agent configuration".into());
            }
            if agent.avatar.as_ref().is_some_and(|a| !bounded(a, 32)) {
                return Err("invalid agent avatar".into());
            }
            match agent.provider {
                Provider::Http => {
                    OpenAiCompatible::new(&agent.endpoint, &agent.model, None)?;
                }
                Provider::Codex => {
                    if !bounded(&agent.model, 64)
                        || !agent
                            .model
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
                    {
                        return Err("invalid codex model name".into());
                    }
                }
            }
        }
        Ok(())
    }
}
