//! The creative part returned by the drafting model, and its acceptance rules.
use crate::brief::{Brief, Language};
use serde::Deserialize;

/// JSON Schema handed to `codex exec --output-schema` (strict structured output).
pub const DRAFT_SCHEMA: &str = r##"{
  "type": "object",
  "additionalProperties": false,
  "required": ["first", "second"],
  "properties": {
    "first": { "$ref": "#/$defs/agent" },
    "second": { "$ref": "#/$defs/agent" }
  },
  "$defs": {
    "agent": {
      "type": "object",
      "additionalProperties": false,
      "required": ["name", "persona", "position"],
      "properties": {
        "name": { "type": "string", "description": "Fictional first name, 2-40 letters" },
        "persona": { "type": "string", "description": "Second-person character sheet, 160-1100 characters" },
        "position": { "type": "string", "description": "Imperative stance instruction, 30-450 characters" }
      }
    }
  }
}"##;

pub const PERSONA_CHARS: (usize, usize) = (160, 1100);
pub const POSITION_CHARS: (usize, usize) = (30, 450);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDraft {
    pub name: String,
    pub persona: String,
    pub position: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub first: AgentDraft,
    pub second: AgentDraft,
}

/// Fixed operator rules appended to every generated persona, independent of model output.
pub fn guardrail(language: Language) -> &'static str {
    match language {
        Language::Tr => {
            "Kurgusal bir karaktersin; gerçek kişileri taklit etme. Fikirleri eleştir, kişilere saldırma. \
             Bilmediğini açıkça söyle; kaynak, istatistik ve alıntı uydurma."
        }
        Language::En => {
            "You are a fictional character; never impersonate real people. Attack ideas, not people. \
             Say plainly when you do not know; never invent sources, statistics or quotations."
        }
    }
}

pub(crate) fn check_name(name: &str) -> Result<(), String> {
    let count = name.chars().count();
    if name != name.trim() || !(2..=40).contains(&count) {
        return Err(format!(
            "name {name:?} must be 2-40 characters without outer spaces"
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'')
    {
        return Err(format!(
            "name {name:?} may contain only letters, spaces, '-' or '\''"
        ));
    }
    Ok(())
}

pub(crate) fn check_text(label: &str, text: &str, (min, max): (usize, usize)) -> Vec<String> {
    let mut errors = Vec::new();
    let count = text.trim().chars().count();
    if !(min..=max).contains(&count) {
        errors.push(format!(
            "{label} has {count} characters; required {min}-{max}"
        ));
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        errors.push(format!("{label} contains control characters"));
    }
    let lower = text.to_lowercase();
    for marker in ["http://", "https://", "www.", "```", "{{", "}}"] {
        if lower.contains(marker) {
            errors.push(format!("{label} must not contain {marker:?}"));
        }
    }
    errors
}

impl Draft {
    pub fn parse(json: &str) -> Result<Self, Vec<String>> {
        serde_json::from_str(json.trim())
            .map_err(|e| vec![format!("output is not valid draft JSON: {e}")])
    }

    /// Returns every rule violation so a retry can address them all at once.
    pub fn validate(&self, brief: &Brief) -> Vec<String> {
        let mut errors = Vec::new();
        let pair = [&self.first, &self.second];
        for (i, (draft, agent)) in pair.iter().zip(&brief.agents).enumerate() {
            let label = if i == 0 { "first" } else { "second" };
            if let Err(e) = check_name(&draft.name) {
                errors.push(format!("{label}.{e}"));
            }
            if let Some(fixed) = &agent.name
                && &draft.name != fixed
            {
                errors.push(format!("{label}.name must be exactly {fixed:?}"));
            }
            errors.extend(check_text(
                &format!("{label}.persona"),
                &draft.persona,
                PERSONA_CHARS,
            ));
            errors.extend(check_text(
                &format!("{label}.position"),
                &draft.position,
                POSITION_CHARS,
            ));
            let other = pair[1 - i];
            if draft.persona.contains(&other.name) {
                errors.push(format!(
                    "{label}.persona must not mention the other character"
                ));
            }
        }
        if self.first.name.to_lowercase() == self.second.name.to_lowercase() {
            errors.push("names must differ".into());
        }
        if self.first.persona.trim() == self.second.persona.trim() {
            errors.push("personas must differ".into());
        }
        if self.first.position.trim() == self.second.position.trim() {
            errors.push("positions must differ".into());
        }
        errors
    }

    /// `(name, persona + guardrail, position)` per agent, in brief order.
    pub(crate) fn into_creative(self, language: Language) -> [(String, String, String); 2] {
        [self.first, self.second].map(|d| {
            let persona = format!("{} {}", d.persona.trim(), guardrail(language));
            (d.name, persona, d.position.trim().to_owned())
        })
    }
}
