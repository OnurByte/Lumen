//! Native OBS composition: the broadcast frame built from OBS's own sources (colour, image,
//! slideshow, FreeType text) and driven live by Lumen, for OBS builds without Browser Source.
//!
//! Every Lumen-owned input is named `Lumen ⁜ …` and lives only in Lumen's own scene
//! collection; setup removes and recreates them so it is idempotent. The live loop sends only
//! changed text and visibility, so mouth flaps are a scene-item toggle (no image reloads).
use lumen_protocol::StageState;
use obws::{
    Client,
    common::{Alignment, BoundsType},
    requests::{
        inputs::{Create, SetSettings},
        scene_items::{Bounds, Position, SceneItemTransform, SetEnabled, SetTransform},
    },
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, time::Instant};

const PREFIX: &str = "Lumen ⁜ ";
const CENTERS: [f32; 2] = [290., 790.];
const PORTRAIT: f32 = 385.;
const PORTRAIT_Y: f32 = 480.;

/// The subset of `~/.config/lumen/theme.toml` the native scene uses.
#[derive(Debug, Clone)]
pub struct Look {
    pub ground: String,
    pub text: String,
    pub muted: String,
    pub surface: String,
    pub border: String,
    pub sides: [String; 2],
    pub question: String,
    pub font: String,
    pub carousel: Vec<PathBuf>,
    pub carousel_seconds: u32,
    pub image_dim: f32,
}

impl Look {
    pub fn load() -> Self {
        let value: toml::Value = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .and_then(|d| std::fs::read_to_string(d.join("lumen/theme.toml")).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or(toml::Value::Table(Default::default()));
        let s = |key: &str, default: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or(default)
                .to_owned()
        };
        let carousel = if value.get("background").and_then(|v| v.as_str()) == Some("carousel") {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            value
                .get("images")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(
                            |p| match (p.strip_prefix("lumen:"), p.strip_prefix("~/"), &home) {
                                (Some(rel), _, _) => lumen_path(rel),
                                (_, Some(rest), Some(home)) => home.join(rest),
                                _ => PathBuf::from(p),
                            },
                        )
                        .filter(|p| p.is_file())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self {
            ground: s("ground", "#000000"),
            text: s("text", "#ffffff"),
            muted: s("muted", "#b3b3b3"),
            surface: s("surface", "#222222"),
            border: s("border", "#404040"),
            sides: [s("side_a", "#ffffff"), s("side_b", "#ffffff")],
            question: s("question", "#ffffff"),
            font: s("font", "Nimbus Sans"),
            carousel,
            carousel_seconds: value
                .get("carousel_seconds")
                .and_then(|v| v.as_integer())
                .map_or(12, |v| v.clamp(3, 600) as u32),
            image_dim: value
                .get("image_dim")
                .and_then(|v| v.as_float())
                .map_or(0.55, |v| v.clamp(0., 0.9) as f32),
        }
    }
}

/// `lumen:rel` -> installed `packs/rel` if present, else the source checkout's.
fn lumen_path(rel: &str) -> PathBuf {
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

/// `#rrggbb` + alpha -> OBS ABGR colour.
pub fn abgr(hex: &str, alpha: f32) -> u32 {
    let n = u32::from_str_radix(hex.trim().trim_start_matches('#'), 16).unwrap_or(0xffffff);
    let (r, g, b) = ((n >> 16) & 255, (n >> 8) & 255, n & 255);
    ((alpha.clamp(0., 1.) * 255.).round() as u32) << 24 | b << 16 | g << 8 | r
}

fn on(hex: &str) -> &'static str {
    let n = u32::from_str_radix(hex.trim().trim_start_matches('#'), 16).unwrap_or(0xffffff);
    let (r, g, b) = (
        ((n >> 16) & 255) as f32,
        ((n >> 8) & 255) as f32,
        (n & 255) as f32,
    );
    if (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255. > 0.55 {
        "#000000"
    } else {
        "#ffffff"
    }
}

fn text_settings(
    look: &Look,
    text: &str,
    size: u32,
    bold: bool,
    color: &str,
    wrap: Option<u32>,
) -> Value {
    let c = abgr(color, 1.);
    let mut s = json!({
        "text": text,
        "font": { "face": look.font, "size": size, "style": if bold { "Bold" } else { "Regular" }, "flags": u8::from(bold) },
        "color1": c,
        "color2": c,
        "antialiasing": true,
        "drop_shadow": false,
        "outline": false,
    });
    if let Some(width) = wrap {
        s["word_wrap"] = json!(true);
        s["custom_width"] = json!(width);
    }
    s
}

fn color_settings(hex: &str, alpha: f32, w: u32, h: u32) -> Value {
    json!({ "color": abgr(hex, alpha), "width": w, "height": h })
}

/// Avatar frame directory for a preset (same lookup as Studio and the overlay).
pub fn avatar_dir(preset: &str) -> Option<PathBuf> {
    if preset.is_empty()
        || !preset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    [
        std::env::var_os("LUMEN_AVATARS").map(PathBuf::from),
        data.map(|d| d.join("lumen/packs/avatars")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/avatars")),
    ]
    .into_iter()
    .flatten()
    .map(|root| root.join(preset))
    .find(|dir| dir.join("idle.png").is_file() && dir.join("talk.png").is_file())
}

struct Element {
    name: String,
    kind: &'static str,
    settings: Value,
    x: f32,
    y: f32,
    align: Alignment,
    bounds: Option<(f32, f32, BoundsType)>,
    enabled: bool,
}

fn el(name: &str, kind: &'static str, settings: Value, x: f32, y: f32) -> Element {
    Element {
        name: format!("{PREFIX}{name}"),
        kind,
        settings,
        x,
        y,
        align: Alignment::TOP | Alignment::LEFT,
        bounds: None,
        enabled: true,
    }
}

fn layout(look: &Look, avatars: &[String; 2]) -> Vec<Element> {
    let mut v = vec![el(
        "zemin",
        "color_source_v3",
        color_settings(&look.ground, 1., 1080, 1920),
        0.,
        0.,
    )];
    if !look.carousel.is_empty() {
        let files: Vec<Value> = look
            .carousel
            .iter()
            .map(
                |p| json!({ "value": p.display().to_string(), "hidden": false, "selected": false }),
            )
            .collect();
        let mut show = el(
            "karosel",
            "slideshow_v2",
            json!({
                "files": files, "slide_time": look.carousel_seconds * 1000, "transition": "fade",
                "transition_speed": 1000, "loop": true, "playback_behavior": "always_play",
            }),
            0.,
            0.,
        );
        show.bounds = Some((1080., 1920., BoundsType::ScaleOuter));
        v.push(show);
        v.push(el(
            "karartma",
            "color_source_v3",
            color_settings("#000000", look.image_dim, 1080, 1920),
            0.,
            0.,
        ));
    }
    v.push(el(
        "soru paneli",
        "color_source_v3",
        color_settings(&look.surface, 0.86, 1000, 320),
        40.,
        135.,
    ));
    v.push(el(
        "soru etiketi",
        "text_ft2_source_v2",
        text_settings(look, "", 33, true, &look.muted, None),
        85.,
        170.,
    ));
    let mut meta = el(
        "stage",
        "text_ft2_source_v2",
        text_settings(look, "", 33, true, &look.muted, None),
        995.,
        170.,
    );
    meta.align = Alignment::TOP | Alignment::RIGHT;
    v.push(meta);
    v.push(el(
        "soru",
        "text_ft2_source_v2",
        text_settings(
            look,
            "Starting soon",
            60,
            false,
            &look.text,
            Some(910),
        ),
        85.,
        220.,
    ));
    for (i, cx) in CENTERS.into_iter().enumerate() {
        let left = cx - PORTRAIT / 2.;
        v.push(el(
            &format!("frame {i}"),
            "color_source_v3",
            color_settings(&look.border, 1., 401, 401),
            left - 8.,
            PORTRAIT_Y - 8.,
        ));
        let mut hl = el(
            &format!("vurgu {i}"),
            "color_source_v3",
            color_settings(&look.sides[i], 1., 401, 401),
            left - 8.,
            PORTRAIT_Y - 8.,
        );
        hl.enabled = false;
        v.push(hl);
        v.push(el(
            &format!("portre zemini {i}"),
            "color_source_v3",
            color_settings(&look.surface, 1., 385, 385),
            left,
            PORTRAIT_Y,
        ));
        if let Some(dir) = avatar_dir(&avatars[i]) {
            for frame in ["idle", "talk", "blink"] {
                let file = dir.join(format!("{frame}.png"));
                if !file.is_file() {
                    continue;
                }
                let mut img = el(
                    &format!("{frame} {i}"),
                    "image_source",
                    json!({ "file": file.display().to_string() }),
                    left,
                    PORTRAIT_Y,
                );
                img.bounds = Some((PORTRAIT, PORTRAIT, BoundsType::ScaleOuter));
                img.enabled = frame == "idle";
                v.push(img);
            }
        }
        let mut dim = el(
            &format!("soluk {i}"),
            "color_source_v3",
            color_settings("#000000", 0.5, 401, 401),
            left - 8.,
            PORTRAIT_Y - 8.,
        );
        dim.enabled = false;
        v.push(dim);
        let centered = |mut e: Element| {
            e.align = Alignment::TOP;
            e
        };
        v.push(centered(el(
            &format!("isim {i}"),
            "text_ft2_source_v2",
            text_settings(look, "", 65, false, &look.text, None),
            cx,
            885.,
        )));
        v.push(el(
            &format!("taraf zemini {i}"),
            "color_source_v3",
            color_settings(&look.sides[i], 1., 440, 70),
            cx - 220.,
            975.,
        ));
        v.push(centered(el(
            &format!("taraf {i}"),
            "text_ft2_source_v2",
            text_settings(look, "", 42, true, on(&look.sides[i]), None),
            cx,
            985.,
        )));
        v.push(centered(el(
            &format!("durum {i}"),
            "text_ft2_source_v2",
            text_settings(look, "", 38, true, &look.sides[i], None),
            cx,
            1065.,
        )));
    }
    let q = on(&look.question);
    for (name, kind, settings, x, y) in [
        (
            "izleyici paneli",
            "color_source_v3",
            color_settings(&look.question, 1., 1000, 190),
            40.,
            QUESTION_Y,
        ),
        (
            "izleyici etiketi",
            "text_ft2_source_v2",
            text_settings(look, "A VIEWER ASKS", 32, true, q, None),
            80.,
            QUESTION_Y + 20.,
        ),
        (
            "izleyici sorusu",
            "text_ft2_source_v2",
            text_settings(look, "", 48, true, q, Some(920)),
            80.,
            QUESTION_Y + 60.,
        ),
    ] {
        let mut e = el(name, kind, settings, x, y);
        e.enabled = false;
        v.push(e);
    }
    for (name, kind, settings, dx, dy) in [
        (
            "caption panel",
            "color_source_v3",
            color_settings("#000000", 0.82, 1000, 420),
            0.,
            0.,
        ),
        (
            "speaker",
            "text_ft2_source_v2",
            text_settings(look, "", 40, true, &look.text, None),
            45.,
            30.,
        ),
        (
            "caption",
            "text_ft2_source_v2",
            text_settings(look, "", 55, true, &look.text, Some(910)),
            45.,
            85.,
        ),
    ] {
        let mut e = el(name, kind, settings, 40. + dx, CAPTION_Y_HIGH + dy);
        e.enabled = false;
        v.push(e);
    }
    v
}

const QUESTION_Y: f32 = 1135.;
const CAPTION_Y_HIGH: f32 = 1135.;
const CAPTION_Y_LOW: f32 = 1350.;

/// Removes Lumen-owned inputs and builds the composition in `scene`.
pub async fn build(
    client: &Client,
    scene: &str,
    look: &Look,
    avatars: &[String; 2],
) -> Result<Driver, String> {
    let e = |what: &'static str| move |err: obws::error::Error| format!("{what}: {err}");
    for input in client
        .inputs()
        .list(None)
        .await
        .map_err(e("kaynak listesi"))?
    {
        if input.id.name.starts_with(PREFIX) {
            client
                .inputs()
                .remove(input.id.name.as_str().into())
                .await
                .map_err(e("eski kaynak"))?;
        }
    }
    // OBS removes sources asynchronously; wait until the names are free again.
    for _ in 0..50 {
        let left = client
            .inputs()
            .list(None)
            .await
            .map_err(e("kaynak listesi"))?;
        if !left.iter().any(|i| i.id.name.starts_with(PREFIX)) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let mut items = HashMap::new();
    for element in layout(look, avatars) {
        let created = client
            .inputs()
            .create(Create {
                scene: scene.into(),
                input: &element.name,
                kind: element.kind,
                settings: Some(element.settings.clone()),
                enabled: Some(element.enabled),
            })
            .await
            .map_err(|err| format!("{}: {err}", element.name))?;
        let item = created.scene_item_id;
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: scene.into(),
                item_id: item,
                transform: SceneItemTransform {
                    position: Some(Position {
                        x: Some(element.x),
                        y: Some(element.y),
                    }),
                    alignment: Some(element.align),
                    bounds: element.bounds.map(|(w, h, t)| Bounds {
                        r#type: Some(t),
                        alignment: Some(Alignment::CENTER),
                        width: Some(w),
                        height: Some(h),
                    }),
                    ..Default::default()
                },
            })
            .await
            .map_err(|err| format!("{} konumu: {err}", element.name))?;
        items.insert(
            element.name.trim_start_matches(PREFIX).to_owned(),
            (item, element.enabled, element.y),
        );
    }
    Ok(Driver {
        scene: scene.to_owned(),
        look: look.clone(),
        items,
        texts: HashMap::new(),
        started: Instant::now(),
    })
}

/// Pushes stage changes into the composition.
pub struct Driver {
    scene: String,
    look: Look,
    /// name -> (scene item id, enabled, base y)
    items: HashMap<String, (i64, bool, f32)>,
    texts: HashMap<String, String>,
    started: Instant,
}

impl Driver {
    async fn text(
        &mut self,
        client: &Client,
        name: &str,
        value: String,
        color: Option<&str>,
    ) -> Result<(), String> {
        let key = format!("{name}{}", color.unwrap_or(""));
        if self.texts.get(name) == Some(&key_value(&key, &value)) {
            return Ok(());
        }
        let mut settings = json!({ "text": value });
        if let Some(c) = color {
            settings["color1"] = json!(abgr(c, 1.));
            settings["color2"] = json!(abgr(c, 1.));
        }
        client
            .inputs()
            .set_settings(SetSettings {
                input: format!("{PREFIX}{name}").as_str().into(),
                settings: &settings,
                overlay: Some(true),
            })
            .await
            .map_err(|e| format!("{name}: {e}"))?;
        self.texts.insert(name.to_owned(), key_value(&key, &value));
        Ok(())
    }

    async fn show(&mut self, client: &Client, name: &str, on: bool) -> Result<(), String> {
        let Some(entry) = self.items.get_mut(name) else {
            return Ok(());
        };
        if entry.1 == on {
            return Ok(());
        }
        client
            .scene_items()
            .set_enabled(SetEnabled {
                scene: self.scene.as_str().into(),
                item_id: entry.0,
                enabled: on,
            })
            .await
            .map_err(|e| format!("{name}: {e}"))?;
        entry.1 = on;
        Ok(())
    }

    async fn move_y(&mut self, client: &Client, name: &str, x: f32, y: f32) -> Result<(), String> {
        let Some(entry) = self.items.get_mut(name) else {
            return Ok(());
        };
        if (entry.2 - y).abs() < 0.5 {
            return Ok(());
        }
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: self.scene.as_str().into(),
                item_id: entry.0,
                transform: SceneItemTransform {
                    position: Some(Position {
                        x: Some(x),
                        y: Some(y),
                    }),
                    ..Default::default()
                },
            })
            .await
            .map_err(|e| format!("{name}: {e}"))?;
        entry.2 = y;
        Ok(())
    }

    pub async fn apply(&mut self, client: &Client, stage: &StageState) -> Result<(), String> {
        let t = self.started.elapsed().as_secs_f32();
        self.text(
            client,
            "soru",
            if stage.finished {
                "That's all for tonight. Thanks for watching.".into()
            } else if stage.topic.is_empty() {
                "Starting soon".into()
            } else {
                stage.topic.clone()
            },
            None,
        )
        .await?;
        let brand = if stage.show.is_empty() {
            String::new()
        } else {
            stage.show.to_uppercase()
        };
        self.text(client, "soru etiketi", brand, None).await?;
        let next = (stage.topic_total > 0)
            .then(|| format!("SORU {} / {}", stage.topic_number, stage.topic_total));
        let meta = [Some(stage.phase.clone()).filter(|p| !p.is_empty()), next]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("   ");
        self.text(client, "stage", meta, None).await?;
        for i in 0..2 {
            let name = &stage.agents[i];
            let speaking = !name.is_empty() && stage.speaker.as_deref() == Some(name.as_str());
            let thinking =
                !speaking && !name.is_empty() && stage.thinking.as_deref() == Some(name.as_str());
            let dimmed = stage.speaker.is_some() && !speaking;
            let talk = speaking && stage.level > 30;
            let blink = !speaking && (t + i as f32 * 1.7) % 4.6 < 0.14;
            self.show(client, &format!("vurgu {i}"), speaking).await?;
            self.show(client, &format!("talk {i}"), talk).await?;
            self.show(client, &format!("blink {i}"), blink && !talk)
                .await?;
            self.show(client, &format!("idle {i}"), !talk && !blink)
                .await?;
            self.show(client, &format!("soluk {i}"), dimmed).await?;
            let look = self.look.clone();
            self.text(
                client,
                &format!("isim {i}"),
                name.clone(),
                Some(if dimmed { &look.muted } else { &look.text }),
            )
            .await?;
            self.text(client, &format!("taraf {i}"), stage.sides[i].clone(), None)
                .await?;
            self.show(
                client,
                &format!("taraf zemini {i}"),
                !stage.sides[i].is_empty(),
            )
            .await?;
            let status = if speaking {
                "● SPEAKING".to_owned()
            } else if thinking {
                format!("thinking{}", ".".repeat(1 + (t * 2.5) as usize % 3))
            } else {
                String::new()
            };
            self.text(
                client,
                &format!("durum {i}"),
                status,
                Some(if thinking {
                    &look.muted
                } else {
                    &look.sides[i]
                }),
            )
            .await?;
        }
        let question = stage.question.clone().unwrap_or_default();
        let has_question = !question.is_empty();
        for name in ["izleyici paneli", "izleyici etiketi", "izleyici sorusu"] {
            self.show(client, name, has_question).await?;
        }
        self.text(
            client,
            "izleyici sorusu",
            question.chars().take(120).collect(),
            None,
        )
        .await?;
        let y = if has_question {
            CAPTION_Y_LOW
        } else {
            CAPTION_Y_HIGH
        };
        self.move_y(client, "caption panel", 40., y).await?;
        self.move_y(client, "speaker", 85., y + 30.).await?;
        self.move_y(client, "caption", 85., y + 85.).await?;
        let captioned = !stage.caption.is_empty();
        for name in ["caption panel", "speaker", "caption"] {
            self.show(client, name, captioned).await?;
        }
        let who = stage.speaker.clone().unwrap_or_default();
        let color = stage
            .agents
            .iter()
            .position(|a| *a == who)
            .map_or(self.look.text.clone(), |i| self.look.sides[i].clone());
        self.text(client, "speaker", who, Some(&color)).await?;
        self.text(client, "caption", stage.caption.clone(), None)
            .await?;
        Ok(())
    }
}

fn key_value(key: &str, value: &str) -> String {
    format!("{key}\u{1}{value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_abgr_with_alpha() {
        assert_eq!(abgr("#ff0000", 1.), 0xff0000ff);
        assert_eq!(abgr("#00ff00", 0.), 0x0000ff00);
        assert_eq!(on("#ffffff"), "#000000");
        assert_eq!(on("#000000"), "#ffffff");
    }

    #[test]
    fn layout_is_unique_prefixed_and_on_canvas() {
        let look = Look::load();
        let items = layout(&look, &["fesli".into(), "ogretmen".into()]);
        let mut names: Vec<_> = items.iter().map(|e| e.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), items.len(), "duplicate source names");
        assert!(items.iter().all(|e| e.name.starts_with(PREFIX)));
        assert!(
            items
                .iter()
                .all(|e| (0. ..=1080.).contains(&e.x) && (0. ..=1920.).contains(&e.y))
        );
        assert!(
            items.iter().any(|e| e.name.ends_with("talk 0")),
            "avatar frames found"
        );
    }
}
