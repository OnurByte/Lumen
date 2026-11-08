//! Editable look of the broadcast frame, stored in `$XDG_CONFIG_HOME/lumen/theme.toml`.
//!
//! Defaults follow the onur.af design language: monochrome (black ground, white text,
//! #b3b3b3 muted, #404040 borders, #222 surfaces), Helvetica-style type with normal-weight
//! headings, 12 px cards and 8 px controls, no decorative accents, glows or gradients.
//! Everything here can be changed from the Görünüm page or by editing the file.
use gpui::{Global, Rgba, rgb};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    Solid,
    Gradient,
    Carousel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub background: Background,
    /// Solid colour, and the gradient's top colour.
    pub ground: String,
    pub gradient_to: String,
    /// Carousel images (absolute paths or `~/...`), shown full-bleed in turn.
    pub images: Vec<String>,
    pub carousel_seconds: u32,
    /// Black veil over images so text stays readable (0 = none, 0.9 = almost black).
    pub image_dim: f32,
    pub text: String,
    pub muted: String,
    pub surface: String,
    pub border: String,
    /// Side colours for the two hosts' frames and labels.
    pub side_a: String,
    pub side_b: String,
    /// Viewer-question card.
    pub question: String,
    pub font: String,
    pub topic_size: f32,
    pub caption_size: f32,
    pub name_size: f32,
    pub radius_card: f32,
    pub radius_control: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // "Hakikat Arayışı": oriental backdrops (carpet, İznik tiles, kilim, lattice) rotate
            // behind a monochrome, portfolio-style frame.
            background: Background::Carousel,
            ground: "#000000".into(),
            gradient_to: "#222222".into(),
            images: DEFAULT_BACKGROUNDS.map(String::from).to_vec(),
            carousel_seconds: 20,
            image_dim: 0.6,
            text: "#ffffff".into(),
            muted: "#b3b3b3".into(),
            surface: "#222222".into(),
            border: "#404040".into(),
            side_a: "#ffffff".into(),
            side_b: "#ffffff".into(),
            question: "#ffffff".into(),
            font: "Nimbus Sans".into(),
            topic_size: 24.,
            caption_size: 22.,
            name_size: 26.,
            radius_card: 12.,
            radius_control: 8.,
        }
    }
}

impl Global for Theme {}

/// Parses `#rrggbb` (falls back to white so a typo never blacks out the stream).
pub fn color(hex: &str) -> Rgba {
    let digits = hex.trim().trim_start_matches('#');
    match u32::from_str_radix(digits, 16) {
        Ok(value) if digits.len() == 6 => rgb(value),
        _ => rgb(0xffffff),
    }
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|dir| dir.join("lumen/theme.toml"))
}

/// Shipped default backdrops, resolved through `lumen:` paths.
pub const DEFAULT_BACKGROUNDS: [&str; 4] = [
    "lumen:backgrounds/oryantal-1.png",
    "lumen:backgrounds/oryantal-2.png",
    "lumen:backgrounds/oryantal-3.png",
    "lumen:backgrounds/oryantal-4.png",
];

/// `lumen:rel` -> installed `packs/rel` if present, else the source checkout's `packs/rel`.
pub fn lumen_path(rel: &str) -> PathBuf {
    let installed = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("lumen/packs").join(rel));
    match installed {
        Some(p) if p.is_file() => p,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs")
            .join(rel),
    }
}

pub fn expand(image: &str) -> PathBuf {
    if let Some(rel) = image.strip_prefix("lumen:") {
        return lumen_path(rel);
    }
    match (image.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(image),
    }
}

impl Theme {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str::<Theme>(&text).ok())
            .map(Theme::sanitized)
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("no config directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// Clamps numbers into ranges that keep the frame readable.
    pub fn sanitized(mut self) -> Self {
        self.carousel_seconds = self.carousel_seconds.clamp(3, 600);
        self.image_dim = if self.image_dim.is_finite() {
            self.image_dim.clamp(0., 0.9)
        } else {
            0.55
        };
        let clamp = |v: f32, lo: f32, hi: f32, default: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            }
        };
        self.topic_size = clamp(self.topic_size, 16., 40., 24.);
        self.caption_size = clamp(self.caption_size, 16., 36., 22.);
        self.name_size = clamp(self.name_size, 16., 40., 26.);
        self.radius_card = clamp(self.radius_card, 0., 32., 12.);
        self.radius_control = clamp(self.radius_control, 0., 24., 8.);
        self.images.truncate(64);
        self
    }

    pub fn images(&self) -> Vec<PathBuf> {
        self.images
            .iter()
            .map(|i| expand(i))
            .filter(|p| p.is_file())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_monochrome_portfolio_tokens_and_round_trip() {
        let theme = Theme::default();
        assert_eq!(
            (theme.ground.as_str(), theme.text.as_str()),
            ("#000000", "#ffffff")
        );
        assert_eq!((theme.radius_card, theme.radius_control), (12., 8.));
        let text = toml::to_string_pretty(&theme).unwrap();
        assert_eq!(toml::from_str::<Theme>(&text).unwrap(), theme);
    }

    #[test]
    fn partial_and_hostile_values_are_repaired() {
        let theme: Theme =
            toml::from_str("background = \"carousel\"\ncaption_size = 900.0").unwrap();
        let theme = theme.sanitized();
        assert_eq!(theme.background, Background::Carousel);
        assert_eq!(theme.caption_size, 36.);
        assert_eq!(theme.ground, "#000000");
        assert_eq!(color("#zzzzzz"), rgb(0xffffff));
        assert_eq!(color("#ff0000"), rgb(0xff0000));
    }
}
