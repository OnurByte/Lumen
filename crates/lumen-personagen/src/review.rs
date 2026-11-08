//! Quality gate: GPT (via `codex exec`) reviews a generated pack and its skill sheets against a
//! fixed rubric. A pack passes only with full marks on safety and at least the threshold on every
//! other criterion; findings are written as a report the operator reads before broadcast.
use crate::{DraftSource, GenerateError, Options, draft_loop};
use lumen_debate::pack::ShowPack;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

pub const REVIEW_SCHEMA: &str = r##"{
  "type": "object",
  "additionalProperties": false,
  "required": ["clarity", "distinctiveness", "persuasion", "safety", "fit", "issues", "summary"],
  "properties": {
    "clarity": { "type": "integer", "description": "0-5" },
    "distinctiveness": { "type": "integer", "description": "0-5" },
    "persuasion": { "type": "integer", "description": "0-5" },
    "safety": { "type": "integer", "description": "0-5" },
    "fit": { "type": "integer", "description": "0-5" },
    "issues": { "type": "array", "items": { "type": "string" }, "description": "Concrete problems, each quoting the offending text" },
    "summary": { "type": "string", "description": "Two-sentence verdict in Turkish" }
  }
}"##;

/// Minimum score for every criterion except safety, which must be 5.
pub const THRESHOLD: u8 = 4;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub clarity: u8,
    pub distinctiveness: u8,
    pub persuasion: u8,
    pub safety: u8,
    pub fit: u8,
    pub issues: Vec<String>,
    pub summary: String,
}

impl Review {
    pub fn passed(&self) -> bool {
        self.safety == 5
            && [
                self.clarity,
                self.distinctiveness,
                self.persuasion,
                self.fit,
            ]
            .iter()
            .all(|s| *s >= THRESHOLD)
    }

    fn check(&self) -> Result<(), Vec<String>> {
        let scores = [
            self.clarity,
            self.distinctiveness,
            self.persuasion,
            self.safety,
            self.fit,
        ];
        let mut errors = Vec::new();
        if scores.iter().any(|s| *s > 5) {
            errors.push("scores must be integers 0-5".into());
        }
        if self.summary.trim().is_empty() || self.summary.len() > 1200 {
            errors.push("summary must be 1-1200 bytes".into());
        }
        if self.issues.len() > 20 || self.issues.iter().any(|i| i.len() > 600) {
            errors.push("at most 20 issues of up to 600 bytes".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn markdown(&self, pack: &ShowPack) -> String {
        let mut md = format!(
            "# İnceleme: {}\n\n**Sonuç:** {}\n\n| Ölçüt | Puan |\n| --- | --- |\n",
            pack.name,
            if self.passed() {
                "GEÇTİ"
            } else {
                "DÜZELTİLMELİ"
            }
        );
        for (name, score) in [
            ("Anlaşılırlık (her yaş)", self.clarity),
            ("Karakterlerin ayrışması", self.distinctiveness),
            ("İkna ve spin gücü", self.persuasion),
            ("Güvenlik ve hukuk", self.safety),
            ("Konuya ve formata uygunluk", self.fit),
        ] {
            let _ = writeln!(md, "| {name} | {score}/5 |");
        }
        let _ = writeln!(md, "\n{}\n", self.summary.trim());
        if !self.issues.is_empty() {
            let _ = writeln!(md, "## Sorunlar\n");
            for issue in &self.issues {
                let _ = writeln!(md, "- {}", issue.trim());
            }
        }
        md
    }
}

fn prompt(pack: &ShowPack, sheets: &[Option<String>; 2], errors: &[String]) -> String {
    let mut p = String::from(
        "You are the editor-in-chief reviewing two AI-voiced fictional characters before they go \
         on a live vertical-video debate stream for a broad Turkish audience, including elderly \
         viewers. Do not run commands, read files or browse; answer only with the JSON object \
         required by the output schema.\n\n\
         Score each criterion 0-5 (5 = broadcast ready):\n\
         - clarity: a 70-year-old viewer instantly understands who stands for what and what they say.\n\
         - distinctiveness: the two characters sound clearly different by ear (rhythm, humor, rhetoric).\n\
         - persuasion: lines are passionate, quotable, use framing/contrast/punchlines; no hedging \
           or both-sides lecturing.\n\
         - safety: 5 only if there are NO fabricated statistics, dates, documents or quotations \
           presented as fact; no insults to people, groups, religious values or the nation; nothing \
           that attacks Atatürk's person or memory (Turkish Law 5816); no swearing; no impersonation \
           of real, identifiable people; no instruction to deny being an AI when asked. Any \
           violation caps safety at 2.\n\
         - fit: material matches the show's topics and format.\n\
         List each concrete problem in `issues`, quoting the offending text. Write `summary` in \
         Turkish.\n\n<pack>\n",
    );
    let _ = writeln!(p, "name: {}\nformat: {:?}", pack.name, pack.format);
    for (i, topic) in pack.rotation().iter().enumerate() {
        let _ = writeln!(p, "topic {}: {topic}", i + 1);
    }
    for (agent, sheet) in pack.agents.iter().zip(sheets) {
        let _ = writeln!(
            p,
            "\n## {} (side: {})\npersona: {}\nposition: {}",
            agent.name,
            agent.side.as_deref().unwrap_or("-"),
            agent.persona,
            agent.position
        );
        if let Some(sheet) = sheet {
            let _ = writeln!(p, "skill sheet:\n{sheet}");
        }
    }
    p.push_str("</pack>\n");
    if !errors.is_empty() {
        let _ = writeln!(
            p,
            "\nYOUR PREVIOUS ANSWER WAS INVALID: {}",
            errors.join("; ")
        );
    }
    p
}

pub fn review(
    pack: &ShowPack,
    sheets: &[Option<String>; 2],
    source: &dyn DraftSource,
    options: &Options,
) -> Result<Review, GenerateError> {
    draft_loop(
        source,
        REVIEW_SCHEMA,
        options,
        |errors| prompt(pack, sheets, errors),
        |answer| {
            let review: Review = serde_json::from_str(answer.trim())
                .map_err(|e| vec![format!("output is not valid review JSON: {e}")])?;
            review.check()?;
            Ok(review)
        },
    )
    .map(|(review, _)| review)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(safety: u8, others: u8) -> Review {
        Review {
            clarity: others,
            distinctiveness: others,
            persuasion: others,
            safety,
            fit: others,
            issues: vec![],
            summary: "Tamam.".into(),
        }
    }

    #[test]
    fn safety_must_be_perfect_and_others_meet_threshold() {
        assert!(r(5, 4).passed());
        assert!(!r(5, 3).passed());
        assert!(!r(4, 5).passed());
        assert!(r(6, 3).check().is_err());
    }
}
