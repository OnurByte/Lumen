//! Broadcast frame and rehearsal room.
//!
//! The frame is our own 9:16 picture that viewers see on the vertical live stream (OBS
//! captures it). It is designed for every age group: one question, two large characters with
//! a plain side label each, the active speaker obvious at a glance, and large high-contrast
//! subtitles. The bottom third and right edge stay clear for the platform's own overlays.
//!
//! The rehearsal room shows the frame next to an operator panel (start/pause, skip, simulated
//! viewers, typed comments). Nothing connects to TikTok; comments go through the same audience
//! ingress a live connector uses. `--stage` opens the clean frame alone.
use crate::theme::{self, Background, Theme};
use crate::{
    link::{self, Link},
    ui::{self, Tone, Variant, color},
};
use gpui::{prelude::*, *};
use gpui_component::input::{Input, InputEvent, InputState};
use lumen_protocol::{AudienceInput, Command, Response, StageState};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

/// Frame design size (9:16); everything inside is laid out in these units and scaled.
const FRAME_W: f32 = 432.;
const FRAME_H: f32 = 768.;
const PANEL_W: f32 = 380.;

/// Portrait scale inside the frame (keeps captions above the platform chat zone).
const PORTRAIT: f32 = 0.84;
const MAX_CHAT: usize = 60;

/// Portrait images for one avatar preset: mouth closed, mouth open, eyes closed.
#[derive(Clone)]
struct Frames {
    idle: PathBuf,
    talk: PathBuf,
    blink: Option<PathBuf>,
}

/// Looks for `<preset>/idle.png` + `talk.png` (+ optional `blink.png`) in, in order:
/// `$LUMEN_AVATARS`, the installed data dir and the source checkout.
fn find_frames(preset: &str) -> Option<Frames> {
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
    let roots = [
        std::env::var_os("LUMEN_AVATARS").map(PathBuf::from),
        data.map(|d| d.join("lumen/packs/avatars")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/avatars")),
    ];
    roots.into_iter().flatten().find_map(|root| {
        let dir = root.join(preset);
        let (idle, talk) = (dir.join("idle.png"), dir.join("talk.png"));
        (idle.is_file() && talk.is_file()).then(|| Frames {
            idle,
            talk,
            blink: Some(dir.join("blink.png")).filter(|b| b.is_file()),
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Frame plus operator panel.
    Rehearsal,
    /// Clean frame only, for capture.
    Stage,
}

#[derive(Clone, PartialEq)]
enum Kind {
    Viewer,
    Own,
    System,
}

struct ChatItem {
    user: String,
    text: String,
    kind: Kind,
}

pub struct Room {
    layout: Layout,
    link: Link,
    response: Option<Response>,
    chat: VecDeque<ChatItem>,
    comment_input: Entity<InputState>,
    bots: bool,
    next_bot: Instant,
    rng: u64,
    seq: u64,
    last_question: Option<String>,
    last_topic: String,
    pub(crate) focus: FocusHandle,
    /// Frame scale for the current render.
    k: f32,
    /// Width taken by UI outside this view (e.g. the app sidebar), and an optional width cap
    /// when the view sits in a narrow pane.
    pub(crate) reserved_w: f32,
    pub(crate) max_w: Option<f32>,
    /// Resolved portrait frames per host (preset name, frames when found).
    frames: [(String, Option<Frames>); 2],
    started: Instant,
}

const VIEWERS: [&str; 18] = [
    "zeynep.k",
    "mert_34",
    "elifsu",
    "maya.dev",
    "sam_uk",
    "kaan.06",
    "derya",
    "onur_k",
    "selin.mp3",
    "emre1903",
    "nora",
    "tolga_x",
    "ipek.art",
    "burak35",
    "gizem.z",
    "cem_dj",
    "melis",
    "ozan.tv",
];

pub fn open(socket: PathBuf, layout: Layout, cx: &mut App) {
    let (w, h, title, app_id) = match layout {
        Layout::Rehearsal => (1180., 860., "Lumen ⁜ Rehearsal", "lumen-rehearsal"),
        Layout::Stage => (540., 960., "Lumen ⁜ Broadcast frame", "lumen-stage-frame"),
    };
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            app_id: Some(app_id.into()),
            window_min_size: Some(size(px(270.), px(480.))),
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(|cx| Room::new(socket, layout, window, cx));
            window.focus(&view.read(cx).focus);
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    if let Err(error) = opened {
        eprintln!("cannot open window: {error}");
    }
}

/// Operator-only session clock: never part of the broadcast frame.
pub(crate) fn session_card(stage: Option<&StageState>) -> Div {
    let Some(stage) = stage else {
        return ui::card("Session", Some("Runtime not connected."));
    };
    let total = stage.session_total_s.max(1);
    let elapsed = stage.session_elapsed_s.min(total);
    let progress = elapsed as f32 / total as f32;
    let status = if stage.finished {
        ("Complete", Tone::Info)
    } else if stage.session_elapsed_s == 0 {
        ("Not started", Tone::Neutral)
    } else {
        ("Running", Tone::Success)
    };
    ui::card("Session", Some("Operator only; never shown on air."))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .items_center()
                .justify_between()
                .child(div().text_2xl().font_family(ui::MONO).child(format!(
                    "{} / {}",
                    hms(elapsed),
                    hms(total)
                )))
                .child(ui::badge(status.0, status.1)),
        )
        .child(
            div()
                .h_2()
                .w_full()
                .rounded_full()
                .bg(color::muted())
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(color::foreground())
                        .w(relative(progress)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .child(ui::stat(
                    "Question",
                    format!("{} / {}", stage.topic_number, stage.topic_total),
                    Tone::Neutral,
                ))
                .child(ui::stat(
                    "Left on question",
                    mmss(stage.topic_remaining_s),
                    if stage.topic_remaining_s < 60 && stage.topic_remaining_s > 0 {
                        Tone::Warning
                    } else {
                        Tone::Neutral
                    },
                ))
                .child(ui::stat(
                    "Left on air",
                    hms(total.saturating_sub(elapsed)),
                    Tone::Neutral,
                )),
        )
}

pub(crate) fn hms(seconds: u32) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn mmss(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

impl Room {
    pub(crate) fn new(
        socket: PathBuf,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                Timer::after(Duration::from_millis(50)).await;
                tick = tick.wrapping_add(1);
                let alive = this.update(cx, |room, cx| {
                    room.pump(tick);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9, |d| d.as_nanos() as u64)
            | 1;
        let comment_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Comment as a viewer, press Enter to send")
        });
        cx.subscribe_in(
            &comment_input,
            window,
            |room, state, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = state.read(cx).value().to_string();
                    room.submit(&text);
                    state.update(cx, |s, cx| s.set_value("", window, cx));
                }
            },
        )
        .detach();
        let mut room = Self {
            layout,
            link: Link::new(socket),
            response: None,
            chat: VecDeque::new(),
            comment_input,
            bots: layout == Layout::Rehearsal,
            next_bot: Instant::now() + Duration::from_secs(2),
            rng: seed,
            seq: 0,
            last_question: None,
            last_topic: String::new(),
            focus: cx.focus_handle(),
            k: 1.,
            reserved_w: 0.,
            max_w: None,
            frames: Default::default(),
            started: Instant::now(),
        };
        room.system("Not connected to a platform. Comments are local and simulated.");
        room.link.send(Command::Status);
        room
    }

    /// 1-based number of the question on air, if a session is running.
    pub(crate) fn current_topic(&self) -> Option<usize> {
        self.response
            .as_ref()
            .map(|r| &r.stage)
            .filter(|s| s.topic_total > 0 && !s.finished)
            .map(|s| usize::from(s.topic_number))
    }

    /// Frame design units -> pixels at the current scale.
    fn u(&self, v: f32) -> Pixels {
        px(v * self.k)
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn push(&mut self, item: ChatItem) {
        self.chat.push_back(item);
        while self.chat.len() > MAX_CHAT {
            self.chat.pop_front();
        }
    }

    fn system(&mut self, text: impl Into<String>) {
        self.push(ChatItem {
            user: "Room".into(),
            text: text.into(),
            kind: Kind::System,
        });
    }

    fn comment(&mut self, user: &str, text: String, own: bool) {
        self.seq += 1;
        self.link.send(Command::Audience(AudienceInput {
            id: format!("rehearsal:{}:{}", std::process::id(), self.seq),
            user: format!("rehearsal:{user}"),
            text: text.clone(),
        }));
        self.push(ChatItem {
            user: user.into(),
            text,
            kind: if own { Kind::Own } else { Kind::Viewer },
        });
    }

    fn running(&self) -> bool {
        self.response
            .as_ref()
            .is_some_and(|r| !r.paused && !r.stopped)
    }

    fn pump(&mut self, tick: u32) {
        for (command, result) in self.link.drain() {
            match result {
                Ok(response) => {
                    if let Some(error) = &response.error
                        && command != Command::Status
                        && !matches!(command, Command::Audience(_))
                    {
                        self.system(format!("Rejected: {error}"));
                    }
                    if self.response.is_none() {
                        self.system("Connected to runtime.");
                    }
                    let stage = &response.stage;
                    if stage.question.is_some() && stage.question != self.last_question {
                        let question: String = stage
                            .question
                            .as_deref()
                            .unwrap_or_default()
                            .chars()
                            .take(90)
                            .collect();
                        self.system(format!("Question on screen: {question}"));
                    }
                    self.last_question = stage.question.clone();
                    if !stage.topic.is_empty() && stage.topic != self.last_topic {
                        if !self.last_topic.is_empty() {
                            self.system(format!("New question: {}", stage.topic));
                        }
                        self.last_topic = stage.topic.clone();
                    }
                    for (slot, preset) in self.frames.iter_mut().zip(&response.stage.avatars) {
                        if slot.0 != *preset {
                            *slot = (preset.clone(), find_frames(preset));
                        }
                    }
                    self.response = Some(response);
                }
                Err(_) => {
                    if self.response.take().is_some() {
                        self.system("Runtime disconnected.");
                    }
                }
            }
        }
        if tick.is_multiple_of(5) && self.link.idle() {
            self.link.send(Command::Status);
        }
        if self.bots && self.running() && Instant::now() >= self.next_bot {
            let text = self.bot_line();
            let user = VIEWERS[(self.rand() % VIEWERS.len() as u64) as usize];
            self.comment(user, text, false);
            let wait = 2_500 + self.rand() % 4_000;
            self.next_bot = Instant::now() + Duration::from_millis(wait);
        }
    }

    fn bot_line(&mut self) -> String {
        let agents = self
            .response
            .as_ref()
            .map(|r| r.stage.agents.clone())
            .unwrap_or_default();
        let a = agents[(self.rand() & 1) as usize].clone();
        let templates = [
            "@{a} buna ne diyorsun?",
            "@{a} can you give an example?",
            "{a} has a point",
            "{a} is struggling this time 😄",
            "Great topic, keep going!",
            "They're both a little right",
            "@{a} what if it was the other way round?",
            "Hold on, isn't that a contradiction?",
            "Can you slow down a bit?",
            "My mum says the same thing 😂",
        ];
        templates[(self.rand() % templates.len() as u64) as usize].replace("{a}", &a)
    }

    fn submit(&mut self, text: &str) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if self.response.is_none() {
            self.system("Runtime not connected; comment not sent.");
            return;
        }
        self.comment("you", text, true);
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if let Some(command) = link::shortcut(event) {
            self.link.send(command);
            cx.stop_propagation();
        }
    }
}

// ---------------------------------------------------------------------------------------
// Broadcast frame
// ---------------------------------------------------------------------------------------

impl Room {
    fn portrait(&self, preset: &str, index: usize, level: f32, speaking: bool) -> AnyElement {
        let t = self.started.elapsed().as_secs_f32();
        let Some(frames) = &self.frames[index].1 else {
            return crate::avatar::draw(preset, index, level, speaking, t, &|v| {
                self.u(v * PORTRAIT)
            })
            .into_any_element();
        };
        let talking = speaking && level > 0.3;
        let blinking = !talking && (t + index as f32 * 1.7) % 4.6 < 0.14;
        let path = if talking {
            &frames.talk
        } else if blinking {
            frames.blink.as_ref().unwrap_or(&frames.idle)
        } else {
            &frames.idle
        };
        // Slight breathing motion; the image is a little larger than its frame so edges never show.
        let side = 176. * PORTRAIT;
        let bob = (t * 1.5 + index as f32).sin() * 1.5;
        div()
            .relative()
            .size(self.u(side))
            .child(
                img(path.clone())
                    .absolute()
                    .left(self.u(-4.))
                    .top(self.u(-4. + bob))
                    .size(self.u(side + 8.))
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element()
    }

    fn host(&self, stage: &StageState, index: usize, theme: &Theme) -> Div {
        let u = |v| self.u(v);
        let name = &stage.agents[index];
        let side = &stage.sides[index];
        let speaking = !name.is_empty() && stage.speaker.as_deref() == Some(name.as_str());
        let someone = stage.speaker.is_some();
        let thinking =
            !speaking && !name.is_empty() && stage.thinking.as_deref() == Some(name.as_str());
        let level = if speaking {
            f32::from(stage.level) / 100.
        } else {
            0.
        };
        let accent = side_color(theme, index);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .gap(u(6.))
            .opacity(if someone && !speaking { 0.5 } else { 1. })
            .child(
                div()
                    .rounded(u(theme.radius_card))
                    .border(u(3.))
                    .border_color(if speaking {
                        accent
                    } else {
                        theme::color(&theme.border)
                    })
                    .child(
                        div()
                            .size(u(176. * PORTRAIT))
                            .rounded(u((theme.radius_card - 3.).max(0.)))
                            .overflow_hidden()
                            .bg(theme::color(&theme.surface))
                            .flex()
                            .justify_center()
                            .child(self.portrait(&stage.avatars[index], index, level, speaking)),
                    ),
            )
            .child(
                div()
                    .text_size(u(theme.name_size))
                    .font_weight(FontWeight::MEDIUM)
                    .child(if name.is_empty() {
                        "…".to_owned()
                    } else {
                        name.clone()
                    }),
            )
            .when(!side.is_empty(), |d| {
                d.child(
                    div()
                        .px(u(10.))
                        .py(u(3.))
                        .rounded(u(theme.radius_control))
                        .bg(accent)
                        .text_color(on(accent))
                        .text_size(u(17.))
                        .font_weight(FontWeight::BOLD)
                        .child(side.clone()),
                )
            })
            .child(
                div()
                    .h(u(24.))
                    .flex()
                    .items_center()
                    .when(thinking, |d| {
                        let dots =
                            ".".repeat(1 + (self.started.elapsed().as_millis() / 400 % 3) as usize);
                        d.child(
                            div()
                                .text_size(u(15.))
                                .text_color(theme::color(&theme.muted))
                                .child(format!("thinking{dots}")),
                        )
                    })
                    .when(speaking, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(u(6.))
                                .text_size(u(15.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(accent)
                                .child(div().size(u(10.)).rounded_full().bg(accent))
                                .child("SPEAKING"),
                        )
                    }),
            )
    }

    /// Ground layer: solid, two-stop gradient, or a cross-fading image carousel with a veil.
    fn ground(&self, theme: &Theme) -> Div {
        let base = div().absolute().inset_0();
        match theme.background {
            Background::Solid => base.bg(theme::color(&theme.ground)),
            Background::Gradient => base.bg(linear_gradient(
                180.,
                linear_color_stop(theme::color(&theme.ground), 0.),
                linear_color_stop(theme::color(&theme.gradient_to), 1.),
            )),
            Background::Carousel => {
                let images = theme.images();
                if images.is_empty() {
                    return base.bg(theme::color(&theme.ground));
                }
                let period = theme.carousel_seconds as f32;
                let t = self.started.elapsed().as_secs_f32();
                let current = (t / period) as usize % images.len();
                let next = (current + 1) % images.len();
                let fade = ((t % period) - (period - 1.)).clamp(0., 1.);
                let layer = |path: &PathBuf, opacity: f32| {
                    img(path.clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(opacity)
                };
                base.bg(theme::color(&theme.ground))
                    .child(layer(&images[current], 1.))
                    .when(images.len() > 1 && fade > 0., |d| {
                        d.child(layer(&images[next], fade))
                    })
                    .child(div().absolute().inset_0().bg(Rgba {
                        r: 0.,
                        g: 0.,
                        b: 0.,
                        a: theme.image_dim,
                    }))
            }
        }
    }

    fn frame(&self, theme: &Theme) -> Div {
        let u = |v| self.u(v);
        let stage = self
            .response
            .as_ref()
            .map(|r| r.stage.clone())
            .unwrap_or_default();
        let has_show = !stage.agents[0].is_empty();
        let topic = if stage.finished {
            "That's all for tonight. Thanks for watching.".to_owned()
        } else if stage.topic.is_empty() {
            "Starting soon".to_owned()
        } else {
            stage.topic.clone()
        };
        // On air: concept name and "SORU n / N" only; timers live in the operator panel.
        let next = (stage.topic_total > 0)
            .then(|| format!("QUESTION {} / {}", stage.topic_number, stage.topic_total));
        let brand = stage.show.to_uppercase();
        let speaker_index = stage
            .speaker
            .as_ref()
            .and_then(|s| stage.agents.iter().position(|a| a == s));
        let muted = theme::color(&theme.muted);
        let panel_bg = Rgba {
            a: 0.86,
            ..theme::color(&theme.surface)
        };
        let caption = (!stage.caption.is_empty()).then(|| {
            let accent = speaker_index.map_or(theme::color(&theme.text), |i| side_color(theme, i));
            div()
                .mx(u(16.))
                .px(u(18.))
                .py(u(14.))
                .rounded(u(theme.radius_card))
                .bg(Rgba {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 0.82,
                })
                .border(u(1.))
                .border_color(theme::color(&theme.border))
                .flex()
                .flex_col()
                .gap(u(4.))
                .child(
                    div()
                        .text_size(u(16.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(accent)
                        .child(stage.speaker.clone().unwrap_or_default()),
                )
                .child(
                    div()
                        .text_size(u(theme.caption_size))
                        .line_height(u(theme.caption_size * 1.3))
                        .font_weight(FontWeight::BOLD)
                        .child(stage.caption.clone()),
                )
        });
        let question = stage.question.clone().map(|q| {
            let bg = theme::color(&theme.question);
            div()
                .mx(u(16.))
                .px(u(16.))
                .py(u(10.))
                .rounded(u(theme.radius_card))
                .bg(bg)
                .text_color(on(bg))
                .flex()
                .flex_col()
                .gap(u(2.))
                .child(
                    div()
                        .text_size(u(13.))
                        .font_weight(FontWeight::BOLD)
                        .child("A VIEWER ASKS"),
                )
                .child(
                    div()
                        .text_size(u(19.))
                        .line_height(u(25.))
                        .font_weight(FontWeight::BOLD)
                        .child(q.chars().take(120).collect::<String>()),
                )
        });
        div()
            .w(u(FRAME_W))
            .h(u(FRAME_H))
            .flex_none()
            .relative()
            .overflow_hidden()
            .font_family(theme.font.clone())
            .text_color(theme::color(&theme.text))
            .child(self.ground(theme))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    // Top ~8% is covered by the platform's header.
                    .pt(u(54.))
                    .gap(u(10.))
                    .child(
                        div()
                            .mx(u(16.))
                            .px(u(18.))
                            .py(u(14.))
                            .rounded(u(theme.radius_card))
                            .bg(panel_bg)
                            .border(u(1.))
                            .border_color(theme::color(&theme.border))
                            .flex()
                            .flex_col()
                            .gap(u(6.))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .text_size(u(13.))
                                    .text_color(muted)
                                    .child(div().font_weight(FontWeight::BOLD).child(brand))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(u(8.))
                                            .when(!stage.phase.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme::color(&theme.text))
                                                        .child(stage.phase.clone()),
                                                )
                                            })
                                            .children(next.map(|n| div().child(n))),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(u(theme.topic_size))
                                    .line_height(u(theme.topic_size * 1.25))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(topic),
                            ),
                    )
                    .when(has_show, |d| {
                        d.child(
                            div()
                                .flex()
                                .px(u(8.))
                                .child(self.host(&stage, 0, theme))
                                .child(self.host(&stage, 1, theme)),
                        )
                    })
                    .children(question)
                    .children(caption),
            )
    }
}

fn side_color(theme: &Theme, index: usize) -> Rgba {
    theme::color(if index == 0 {
        &theme.side_a
    } else {
        &theme.side_b
    })
}

/// Black or white text, whichever reads on `bg`.
fn on(bg: Rgba) -> Rgba {
    if 0.2126 * bg.r + 0.7152 * bg.g + 0.0722 * bg.b > 0.55 {
        rgb(0x000000)
    } else {
        rgb(0xffffff)
    }
}

// ---------------------------------------------------------------------------------------
// Operator panel (rehearsal only; never part of the broadcast frame)
// ---------------------------------------------------------------------------------------

impl Room {
    fn panel(&self, cx: &mut Context<Self>) -> Div {
        let state = match &self.response {
            None => ("Not connected", Tone::Danger),
            Some(r) if r.stopped => ("Emergency stopped", Tone::Danger),
            Some(r) if r.paused => ("Paused", Tone::Warning),
            Some(_) => ("On air", Tone::Success),
        };
        let connected = self.response.is_some();
        let running = self.running();
        let control = |id: &'static str, text: &str, keys: &str, command: Command, variant| {
            let enabled = connected || command == Command::EmergencyStop;
            ui::button(id, text, keys, variant, enabled)
                .on_click(cx.listener(move |room, _, _, _| room.link.send(command.clone())))
        };
        let session = session_card(self.response.as_ref().map(|r| &r.stage));
        let show = ui::card("Broadcast", Some("Shortcuts work anywhere in this window.")).child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(if running {
                    control("pause", "Pause", "F2", Command::Pause, Variant::Secondary)
                } else {
                    control("resume", "Start", "F3", Command::Resume, Variant::Active)
                })
                .child(control(
                    "topic",
                    "Next question",
                    "F9",
                    Command::SkipTopic,
                    Variant::Secondary,
                ))
                .child(control(
                    "turn",
                    "Hand over the floor",
                    "F8",
                    Command::SkipTurn,
                    Variant::Secondary,
                ))
                .child(control(
                    "emergency",
                    "Emergency stop",
                    "Ctrl ⇧ Esc",
                    Command::EmergencyStop,
                    Variant::Destructive,
                )),
        );
        let bots = self.bots;
        let audience = ui::card(
            "Audience rehearsal",
            Some("Not connected to a platform. Comments enter the real question queue."),
        )
        .child(
            div()
                .id("bots")
                .flex()
                .items_center()
                .justify_between()
                .h_9()
                .px_3()
                .rounded_md()
                .border_1()
                .border_color(color::input())
                .cursor_pointer()
                .hover(|s| s.bg(color::muted()))
                .text_sm()
                .child("Simulated viewers")
                .child(ui::badge(
                    if bots { "On" } else { "Off" },
                    if bots { Tone::Success } else { Tone::Neutral },
                ))
                .on_click(cx.listener(|room, _, _, _| room.bots = !room.bots)),
        )
        .child(Input::new(&self.comment_input).cleanable(true))
        .child(
            div()
                .text_xs()
                .text_color(color::subtle_foreground())
                .child("Tip: start with \"@Name\" to ask that host directly."),
        );
        let start = self.chat.len().saturating_sub(12);
        let feed = ui::card("Chat", None).child(div().flex().flex_col().gap_2().children(
            self.chat.iter().skip(start).map(|item| {
                let name_color = match item.kind {
                    Kind::System => color::warning(),
                    Kind::Own => color::info(),
                    Kind::Viewer => color::muted_foreground(),
                };
                div()
                    .flex()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(name_color)
                            .child(item.user.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(if item.kind == Kind::System {
                                color::muted_foreground()
                            } else {
                                color::foreground()
                            })
                            .child(item.text.clone()),
                    )
            }),
        ));
        div()
            .w(px(PANEL_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .bg(color::background())
            .border_l_1()
            .border_color(color::border())
            .font_family(ui::FONT)
            .text_color(color::foreground())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Rehearsal"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(color::muted_foreground())
                                    .child("The frame on the left is exactly what goes on air."),
                            ),
                    )
                    .child(ui::badge(state.0, state.1)),
            )
            .child(session)
            .child(show)
            .child(audience)
            .child(feed)
    }
}

impl Render for Room {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let rehearsal = self.layout == Layout::Rehearsal;
        let panel = if rehearsal { PANEL_W } else { 0. };
        let margin = if rehearsal { 24. } else { 0. };
        let mut avail_w = f32::from(viewport.width) - self.reserved_w - panel - 2. * margin;
        if let Some(max) = self.max_w {
            avail_w = avail_w.min(max - 2. * margin);
        }
        let avail_w = avail_w.max(100.);
        let avail_h = (f32::from(viewport.height) - 2. * margin).max(100.);
        self.k = (avail_w / FRAME_W).min(avail_h / FRAME_H);
        let theme = cx.try_global::<Theme>().cloned().unwrap_or_default();
        div()
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .bg(rgb(0x000000))
            .on_key_down(cx.listener(|room, event: &KeyDownEvent, _, cx| room.on_key(event, cx)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.frame(&theme)),
            )
            .when(rehearsal, |d| d.child(self.panel(cx)))
    }
}
