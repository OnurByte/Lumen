//! Theme colour tokens edited on the Appearance page, hex helpers and Lumen's featured tones.
use crate::theme::Theme;

/// Theme colours the picker edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Ground,
    GradientTo,
    Text,
    Muted,
    Surface,
    Border,
    SideA,
    SideB,
    Question,
}

impl Token {
    pub const ALL: [Token; 9] = [
        Token::Ground,
        Token::GradientTo,
        Token::Text,
        Token::Muted,
        Token::Surface,
        Token::Border,
        Token::SideA,
        Token::SideB,
        Token::Question,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Token::Ground => "Background",
            Token::GradientTo => "Gradient end",
            Token::Text => "Text",
            Token::Muted => "Secondary text",
            Token::Surface => "Panel",
            Token::Border => "Border",
            Token::SideA => "Side A",
            Token::SideB => "Side B",
            Token::Question => "Viewer question card",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Token::Ground => "Behind everything; under images in carousel mode",
            Token::GradientTo => "Bottom color in gradient mode",
            Token::Text => "Question, names and subtitles",
            Token::Muted => "Labels and secondary lines",
            Token::Surface => "Question panel and portrait backing",
            Token::Border => "Panel and portrait outlines",
            Token::SideA => "First host: frame, label, speaking marker",
            Token::SideB => "Second host: frame, label, speaking marker",
            Token::Question => "Card shown when a viewer question is on air",
        }
    }

    pub fn get(self, t: &Theme) -> &str {
        match self {
            Token::Ground => &t.ground,
            Token::GradientTo => &t.gradient_to,
            Token::Text => &t.text,
            Token::Muted => &t.muted,
            Token::Surface => &t.surface,
            Token::Border => &t.border,
            Token::SideA => &t.side_a,
            Token::SideB => &t.side_b,
            Token::Question => &t.question,
        }
    }

    pub fn set(self, t: &mut Theme, hex: String) {
        let slot = match self {
            Token::Ground => &mut t.ground,
            Token::GradientTo => &mut t.gradient_to,
            Token::Text => &mut t.text,
            Token::Muted => &mut t.muted,
            Token::Surface => &mut t.surface,
            Token::Border => &mut t.border,
            Token::SideA => &mut t.side_a,
            Token::SideB => &mut t.side_b,
            Token::Question => &mut t.question,
        };
        *slot = hex;
    }
}

/// Lumen tones shown above the kit's full palette; 11 fit the picker popover's row.
pub const FEATURED: [&str; 11] = [
    "#000000", "#ffffff", "#222222", "#8b1e2d", "#e30a17", "#d98e04", "#c9a227", "#1d4e89",
    "#0e7c86", "#1b7f5b", "#e8d5b5",
];

/// `#rrggbb`, `rrggbb` or the short `#rgb` form.
pub fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.trim().trim_start_matches('#');
    let digits: String = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect(),
        6 => digits.to_owned(),
        _ => return None,
    };
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(&digits, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

pub fn to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_palette_is_valid() {
        for hex in FEATURED {
            let (r, g, b) = parse_hex(hex).expect(hex);
            assert_eq!(to_hex(r, g, b), hex);
        }
        assert!(parse_hex("#12345").is_none() && parse_hex("zzzzzz").is_none());
        assert_eq!(parse_hex("F80"), Some((0xff, 0x88, 0x00)));
        assert!(parse_hex("+12345").is_none());
    }

    #[test]
    fn every_token_reads_and_writes_its_own_slot() {
        let mut theme = Theme::default();
        for (i, token) in Token::ALL.into_iter().enumerate() {
            let hex = format!("#0000{i:02x}");
            token.set(&mut theme, hex.clone());
            assert_eq!(token.get(&theme), hex);
        }
    }
}
