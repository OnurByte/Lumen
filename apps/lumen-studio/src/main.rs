//! Operator control room; the daemon owns the show lifetime.
mod product;
mod accounts;
mod app;
mod avatar;
mod link;
mod palette;
mod rehearsal;
mod session;
mod theme;
mod ui;

use gpui::{prelude::*, *};
use link::Link;
use lumen_protocol::{Command, Response};
use std::{collections::VecDeque, path::PathBuf, time::Duration};
use ui::{Tone, Variant, color};

pub(crate) struct Studio {
    link: Link,
    response: Option<Response>,
    activity: VecDeque<String>,
    /// Lines heard on air (speaker, text), newest last.
    transcript: VecDeque<(String, String)>,
    pub(crate) focus: FocusHandle,
}

impl Studio {
    pub(crate) fn new(socket: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let mut tick = 0u8;
            loop {
                Timer::after(Duration::from_millis(250)).await;
                if this
                    .update(cx, |studio, cx| {
                        for (command, result) in studio.link.drain() {
                            if matches!(command, Command::Audience(_)) {
                                continue;
                            }
                            match result {
                                Ok(response) => {
                                    if let Some(error) = &response.error {
                                        studio.note(error.clone());
                                    } else if command != Command::Status {
                                        studio.note(format!("{} applied", label(&command)));
                                    } else if studio.response.is_none() {
                                        studio.note("Connected to runtime".into());
                                    }
                                    let stage = &response.stage;
                                    if !stage.caption.is_empty()
                                        && studio.transcript.back().map(|(_, t)| t)
                                            != Some(&stage.caption)
                                    {
                                        studio.transcript.push_back((
                                            stage.speaker.clone().unwrap_or_default(),
                                            stage.caption.clone(),
                                        ));
                                        while studio.transcript.len() > 12 {
                                            studio.transcript.pop_front();
                                        }
                                    }
                                    studio.response = Some(response);
                                }
                                Err(error) => {
                                    if studio.response.take().is_some()
                                        || studio.activity.is_empty()
                                    {
                                        studio.note(format!("Disconnected: {error}"));
                                    }
                                }
                            }
                            cx.notify();
                        }
                        tick = (tick + 1) % 4;
                        if tick == 0 && studio.link.idle() {
                            studio.send(Command::Status);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let studio = Self {
            link: Link::new(socket),
            response: None,
            activity: VecDeque::new(),
            transcript: VecDeque::new(),
            focus: cx.focus_handle(),
        };
        studio.send(Command::Status);
        studio
    }
    fn note(&mut self, message: String) {
        if self.activity.front() != Some(&message) {
            self.activity.push_front(message.clone());
            self.activity.truncate(6);
        }
    }
    fn send(&self, command: Command) {
        self.link.send(command);
    }
}

fn label(command: &Command) -> &'static str {
    match command {
        Command::Pause => "Pause",
        Command::Resume => "Resume",
        Command::MuteAll => "Mute",
        Command::SkipTurn => "Hand over the floor",
        Command::SkipTopic => "Next question",
        Command::StopStream => "Stop stream",
        Command::EmergencyStop => "Emergency stop",
        Command::Status => "Status",
        Command::Audience(_) => "Viewer message",
        Command::SetTopics(_) => "Question list",
    }
}

impl Studio {
    fn header(&self, state: (&'static str, Tone)) -> Div {
        let connected = self.response.is_some();
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_6()
            .h(px(56.))
            .flex_none()
            .border_b_1()
            .border_color(color::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(ui::mark(24.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Control"),
                    )
                    .child(div().text_sm().text_color(color::input()).child("/"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(color::muted_foreground())
                            .child("Live debate control room"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::badge(
                        if connected {
                            "Connected"
                        } else {
                            "Not connected"
                        },
                        if connected {
                            Tone::Success
                        } else {
                            Tone::Danger
                        },
                    ))
                    .child(ui::badge(state.0, state.1)),
            )
    }

    fn overview(&self, state: (&'static str, Tone)) -> Div {
        let response = self.response.as_ref();
        let paused = response.is_some_and(|r| r.paused);
        let muted = response.is_some_and(|r| r.muted);
        let stopped = response.is_some_and(|r| r.stopped);
        let hint = match response {
            None => "Waiting for the lumend control socket. Start the daemon to take control.",
            Some(_) if stopped => {
                "Emergency stop is latched. Restart the runtime to go live again."
            }
            Some(_) if paused => "Show is paused. Press F3 to resume the debate.",
            Some(_) => "On air. Controls respond instantly, even while models are generating.",
        };
        let speaker = response
            .and_then(|r| r.stage.speaker.clone())
            .unwrap_or_else(|| "Idle".into());
        div()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .rounded_xl()
            .bg(color::card())
            .border_1()
            .border_color(color::border())
            .shadow_sm()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(ui::label("Broadcast status"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().size_2p5().rounded_full().bg(state.1.color()))
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(state.0),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(color::muted_foreground())
                            .child(hint),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(ui::stat(
                        "Runtime",
                        if response.is_some() {
                            "Connected"
                        } else {
                            "Not connected"
                        },
                        if response.is_some() {
                            Tone::Success
                        } else {
                            Tone::Danger
                        },
                    ))
                    .child(ui::stat(
                        "Audio",
                        if muted { "Muted" } else { "On" },
                        if muted { Tone::Warning } else { Tone::Success },
                    ))
                    .child(ui::stat(
                        "Floor",
                        speaker,
                        if response.is_some_and(|r| r.stage.speaker.is_some()) {
                            Tone::Info
                        } else {
                            Tone::Neutral
                        },
                    )),
            )
    }

    fn controls(&self, cx: &mut Context<Self>) -> Div {
        let response = self.response.as_ref();
        let connected = response.is_some();
        let paused = response.is_some_and(|r| r.paused);
        let muted = response.is_some_and(|r| r.muted);
        let rows: [(&'static str, &str, &str, Command, Variant); 6] = [
            (
                "pause",
                "Pause",
                "F2",
                Command::Pause,
                if paused {
                    Variant::Active
                } else {
                    Variant::Secondary
                },
            ),
            (
                "resume",
                "Resume",
                "F3",
                Command::Resume,
                Variant::Secondary,
            ),
            (
                "mute",
                if muted { "Unmute" } else { "Mute" },
                "F4",
                Command::MuteAll,
                if muted {
                    Variant::Active
                } else {
                    Variant::Secondary
                },
            ),
            (
                "skip",
                "Hand over the floor",
                "F8",
                Command::SkipTurn,
                Variant::Secondary,
            ),
            (
                "topic",
                "Next question",
                "F9",
                Command::SkipTopic,
                Variant::Secondary,
            ),
            (
                "stream",
                "Stop stream",
                "F10",
                Command::StopStream,
                Variant::Outline,
            ),
        ];
        let action = |id, text: &str, keys: &str, command: Command, variant, enabled| {
            let keyboard = command.clone();
            ui::button(id, text, keys, variant, enabled)
                .on_click(cx.listener(move |studio, _, _, _| studio.send(command.clone())))
                .on_key_down(cx.listener(move |studio, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        studio.send(keyboard.clone());
                        cx.stop_propagation();
                    }
                }))
        };
        let mut card = ui::card("Controls", Some("Shortcuts work anywhere in this window."));
        let mut list = div().flex().flex_col().gap_2();
        for (index, (id, text, keys, command, variant)) in rows.into_iter().enumerate() {
            if index == 3 || index == 5 {
                list = list.child(div().py_1().child(ui::separator()));
            }
            list = list.child(action(id, text, keys, command, variant, connected));
        }
        card = card.child(list).child(ui::separator()).child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(action(
                    "emergency",
                    "Emergency stop",
                    "Ctrl ⇧ Esc",
                    Command::EmergencyStop,
                    Variant::Destructive,
                    true,
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(color::subtle_foreground())
                        .child("Cuts audio, cancels generation and stays latched until restart."),
                ),
        );
        card
    }

    fn question_card(&self, cx: &mut Context<Self>) -> Div {
        let stage = self.response.as_ref().map(|r| &r.stage);
        let current = stage
            .filter(|s| s.topic_total > 0)
            .map(|s| usize::from(s.topic_number));
        let finished = stage.is_some_and(|s| s.finished);
        let questions = cx
            .try_global::<session::Broadcast>()
            .map(|b| b.session.questions.clone())
            .unwrap_or_default();
        let mut list = div().flex().flex_col().gap_1();
        for (i, q) in questions.iter().enumerate() {
            let n = i + 1;
            let (mark, tone_color, weight) = match current {
                _ if finished => ("✓", color::subtle_foreground(), FontWeight::NORMAL),
                Some(c) if n < c => ("✓", color::subtle_foreground(), FontWeight::NORMAL),
                Some(c) if n == c => ("●", color::foreground(), FontWeight::BOLD),
                _ => ("○", color::muted_foreground(), FontWeight::NORMAL),
            };
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .when(current == Some(n) && !finished, |d| d.bg(color::muted()))
                    .child(
                        div()
                            .w(px(18.))
                            .flex_none()
                            .text_sm()
                            .text_color(tone_color)
                            .child(mark),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(weight)
                            .text_color(tone_color)
                            .child(format!("{n}. {q}")),
                    ),
            );
        }
        ui::card(
            "Questions",
            Some("Done ones are ticked, the live one is highlighted. Edit them on Questions."),
        )
        .child(if questions.is_empty() {
            div()
                .text_sm()
                .text_color(color::subtle_foreground())
                .child("No questions yet.")
        } else {
            list
        })
    }

    fn obs_card(&self, cx: &mut Context<Self>) -> Div {
        let obs = cx
            .try_global::<session::Broadcast>()
            .map(|b| b.obs.clone())
            .unwrap_or_default();
        let (label, tone) = if obs.streaming {
            ("Live", Tone::Danger)
        } else if obs.connected {
            ("Ready", Tone::Success)
        } else if obs.obs_running {
            ("Connecting", Tone::Warning)
        } else {
            ("Off", Tone::Neutral)
        };
        ui::card("OBS", None)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::badge(label, tone))
                    .child(div().font_family(ui::MONO).text_sm().child(rehearsal::hms(
                        obs.stream_seconds.min(u64::from(u32::MAX)) as u32,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(ui::stat(
                        "Congestion",
                        format!("{:.0}%", obs.congestion * 100.),
                        Tone::Neutral,
                    ))
                    .child(ui::stat(
                        "Recording",
                        if obs.recording { "On" } else { "Off" },
                        Tone::Neutral,
                    )),
            )
    }

    fn transcript_card(&self) -> Div {
        let lines = self.transcript.iter().rev().take(6).map(|(who, text)| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(color::muted_foreground())
                        .child(who.clone()),
                )
                .child(div().text_sm().child(text.clone()))
        });
        ui::card("Transcript", Some("Latest lines heard on air."))
            .child(div().flex().flex_col().gap_3().children(lines))
    }

    fn activity(&self) -> Div {
        let items = if self.activity.is_empty() {
            vec![
                div()
                    .text_xs()
                    .text_color(color::subtle_foreground())
                    .child("Connecting to runtime…"),
            ]
        } else {
            self.activity
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(div().mt_1().flex_none().size_1p5().rounded_full().bg(
                            if index == 0 {
                                color::foreground()
                            } else {
                                color::input()
                            },
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(if index == 0 {
                                    color::foreground()
                                } else {
                                    color::muted_foreground()
                                })
                                .child(entry.clone()),
                        )
                })
                .collect()
        };
        ui::card("Activity", None).child(div().flex().flex_col().gap_2().children(items))
    }
}

impl Render for Studio {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = match &self.response {
            Some(r) if r.stopped => ("Emergency stopped", Tone::Danger),
            Some(r) if r.paused && r.stage.finished => ("Session complete", Tone::Info),
            Some(r) if r.paused => ("Paused", Tone::Warning),
            Some(_) => ("On air", Tone::Success),
            None => ("Not connected", Tone::Neutral),
        };
        div()
            .size_full()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .bg(color::background())
            .font_family(ui::FONT)
            .text_color(color::foreground())
            .on_key_down(cx.listener(|studio, event: &KeyDownEvent, _, cx| {
                let command = link::shortcut(event);
                if let Some(command) = command {
                    studio.send(command);
                    cx.stop_propagation();
                }
            }))
            .child(self.header(state))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap_5()
                    .p_6()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(self.overview(state))
                            .child(rehearsal::session_card(
                                self.response.as_ref().map(|r| &r.stage),
                            ))
                            .child(self.question_card(cx)),
                    )
                    .child(
                        div()
                            .w(px(320.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(self.controls(cx))
                            .child(self.obs_card(cx))
                            .child(self.transcript_card())
                            .child(self.activity()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .h_10()
                    .flex_none()
                    .border_t_1()
                    .border_color(color::border())
                    .text_xs()
                    .text_color(color::subtle_foreground())
                    .child(format!("Socket  {}", self.link.socket.display()))
                    .child("Tab and Enter operate controls ⁜ Ctrl K finds anything"),
            )
    }
}
/// gpui-component's theme, tuned to the onur.af tokens used across Lumen.
fn kit_theme(cx: &mut App) {
    use gpui_component::{Theme as Kit, ThemeMode};
    Kit::change(ThemeMode::Dark, None, cx);
    let kit = Kit::global_mut(cx);
    kit.font_family = ui::FONT.into();
    kit.mono_font_family = ui::MONO.into();
    kit.radius = px(8.);
    kit.radius_lg = px(12.);
    kit.shadow = false;
    let c = &mut kit.colors;
    let hsla = |hex: u32| -> Hsla { rgb(hex).into() };
    c.background = hsla(0x000000);
    c.foreground = hsla(0xffffff);
    c.border = hsla(0x404040);
    c.input = hsla(0x404040);
    c.muted = hsla(0x222222);
    c.muted_foreground = hsla(0xb3b3b3);
    c.popover = hsla(0x0d0d0d);
    c.primary = hsla(0xffffff);
    c.primary_foreground = hsla(0x000000);
    c.secondary = hsla(0x222222);
    c.accent = hsla(0x2e2e2e);
    c.ring = hsla(0xffffff);
}

fn main() {
    lumen_protocol::prepare_environment();
    let mut only = None;
    let mut socket = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--rehearsal" => only = Some(rehearsal::Layout::Rehearsal),
            "--stage" => only = Some(rehearsal::Layout::Stage),
            _ => socket = Some(PathBuf::from(arg)),
        }
    }
    let socket = socket.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into()))
            .join("lumen/control.sock")
    });
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        kit_theme(cx);
        cx.set_global(theme::Theme::load());
        cx.bind_keys([KeyBinding::new("secondary-k", app::ToggleFinder, None)]);
        if let Some(layout) = only {
            // Stand-alone windows for capture (`--stage`) or an existing runtime.
            rehearsal::open(socket, layout, cx);
            cx.activate(true);
            return;
        }
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Lumen".into()),
                        ..Default::default()
                    }),
                    app_id: Some("org.lumen.Lumen".into()),
                    window_min_size: Some(size(px(1000.), px(640.))),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| app::LumenApp::new(socket, window, cx));
                    let focus = view.read(cx).focus_handle_for_rehearsal(cx);
                    window.focus(&focus);
                    window.on_window_should_close(cx, |_, cx| {
                        cx.quit();
                        true
                    });
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )
            .expect("open Lumen window");
        let _ = window;
        cx.activate(true);
    });
}
