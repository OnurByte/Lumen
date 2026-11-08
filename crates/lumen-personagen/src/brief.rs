//! Operator-written show brief: everything except the creative persona text.
//!
//! Runtime-sensitive settings (model, endpoint, voice, budgets, key variable name) are
//! copied verbatim into the generated pack and are never delegated to the drafting model.
use lumen_debate::{
    director::Format,
    pack::{AgentConfig, ShowPack},
};
use serde::Deserialize;

pub const MAX_BRIEF_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    pub name: String,
    pub topic: String,
    /// Further punchy topics rotated after `topic` (copied verbatim into the pack).
    #[serde(default)]
    pub topics: Vec<String>,
    /// Minutes per topic before rotating (default 10).
    #[serde(default = "default_topic_minutes")]
    pub topic_minutes: u16,
    /// Shared skill sheets for both agents, e.g. ["spin-doctor"].
    #[serde(default)]
    pub skills: Vec<String>,
    pub format: Format,
    /// `tr` or `en`; the language personas and positions are written in.
    #[serde(default = "default_language")]
    pub language: Language,
    /// Overall show tone, e.g. "esprili, saygılı, hızlı".
    pub tone: String,
    /// Optional free-form direction for the writer (audience, era, running gags...).
    #[serde(default)]
    pub notes: Option<String>,
    pub agents: [AgentBrief; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Tr,
    En,
}

fn default_topic_minutes() -> u16 {
    10
}

fn default_language() -> Language {
    Language::Tr
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentBrief {
    /// Fixed character name; when absent the writer invents a fictional one.
    #[serde(default)]
    pub name: Option<String>,
    /// The stance this agent is assigned for the topic (becomes `position`).
    pub stance: String,
    /// Short on-screen side label (max 28 characters), e.g. "Liderler belirler".
    #[serde(default)]
    pub side: Option<String>,
    /// Optional character direction (temperament, quirks, background archetype).
    #[serde(default)]
    pub character: Option<String>,
    pub voice: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub provider: lumen_debate::pack::Provider,
    pub model: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    pub temperature: f32,
    pub max_tokens: u16,
}

impl Brief {
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > MAX_BRIEF_BYTES {
            return Err("brief too large".into());
        }
        let brief: Self = toml::from_str(source).map_err(|e| format!("invalid brief TOML: {e}"))?;
        brief.check()?;
        Ok(brief)
    }

    fn check(&self) -> Result<(), String> {
        let short = |s: &str, limit: usize| !s.trim().is_empty() && s.chars().count() <= limit;
        if !short(&self.tone, 200) {
            return Err("brief tone must be 1..200 characters".into());
        }
        if self
            .notes
            .as_deref()
            .is_some_and(|n| n.chars().count() > 2000)
        {
            return Err("brief notes exceed 2000 characters".into());
        }
        for (i, agent) in self.agents.iter().enumerate() {
            if !short(&agent.stance, 500) {
                return Err(format!("agent {}: stance must be 1..500 characters", i + 1));
            }
            if agent.character.as_deref().is_some_and(|c| !short(c, 1000)) {
                return Err(format!(
                    "agent {}: character must be 1..1000 characters",
                    i + 1
                ));
            }
            if let Some(name) = &agent.name {
                crate::draft::check_name(name).map_err(|e| format!("agent {}: {e}", i + 1))?;
            }
        }
        if let [Some(a), Some(b)] = [&self.agents[0].name, &self.agents[1].name]
            && a.to_lowercase() == b.to_lowercase()
        {
            return Err("agent names must differ".into());
        }
        // Validate runtime settings up front with placeholder creative text, so a bad
        // endpoint or budget fails before any drafting call is spent.
        self.assemble(
            ["Placeholder A", "Placeholder B"].map(|n| (n.into(), "x".into(), "x".into())),
        )
        .check()
        .map_err(|e| format!("brief runtime settings: {e}"))
    }

    /// Combines creative `(name, persona, position)` text with operator settings.
    pub(crate) fn assemble(&self, creative: [(String, String, String); 2]) -> ShowPack {
        let [a, b] = creative;
        let agent =
            |brief: &AgentBrief, (name, persona, position): (String, String, String)| AgentConfig {
                name,
                persona,
                position,
                side: brief.side.clone(),
                avatar: brief.avatar.clone(),
                provider: brief.provider,
                voice: brief.voice.clone(),
                model: brief.model.clone(),
                endpoint: brief.endpoint.clone(),
                api_key_env: brief.api_key_env.clone(),
                temperature: brief.temperature,
                max_tokens: brief.max_tokens,
            };
        ShowPack {
            name: self.name.clone(),
            topic: self.topic.clone(),
            topics: self.topics.clone(),
            topic_minutes: self.topic_minutes,
            skills: self.skills.clone(),
            format: self.format,
            agents: [agent(&self.agents[0], a), agent(&self.agents[1], b)],
        }
    }
}
