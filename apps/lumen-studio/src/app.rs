//! The single Lumen window: sidebar navigation (Prova, Kontrol, Görünüm), pack and mode
//! selection, and the show runtime (`lumend`) started and stopped by the app itself.
use crate::{
    Studio,
    link::Link,
    palette,
    rehearsal::{self, Layout, Room},
    session::{self, Broadcast, Session},
    theme::{self, Background, Theme},
    ui::{self, Tone, color},
};
use gpui::{prelude::*, *};
use gpui_component::{
    Sizable,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{Input, InputEvent, InputState},
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

const SIDEBAR_W: f32 = 232.;
const PREVIEW_W: f32 = 420.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Rehearsal,
    Control,
    Look,
    Broadcast,
    Questions,
    Accounts,
}

/// Port of the overlay page `lumend` serves for OBS (see `LUMEN_OVERLAY_PORT`).
fn overlay_port() -> u16 {
    std::env::var("LUMEN_OVERLAY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(47800)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Rehearsal with the pack's models (GPT via codex).
    Models,
    /// Rehearsal with skill-sheet sample lines; no model needed.
    Scripted,
    /// Live: models, TTS and audio device.
    Live,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Models => "GPT rehearsal",
            Mode::Scripted => "Scripted rehearsal",
            Mode::Live => "Live broadcast",
        }
    }
}

/// `lumend` child process owned by the window; killed when the app closes.
struct Runtime {
    child: Option<Child>,
    status: String,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("LUMEN_HOME") {
        return PathBuf::from(home);
    }
    let installed = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("lumen"));
    match installed {
        Some(dir) if dir.join("packs/personas").is_dir() => dir,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    }
}

fn packs() -> Vec<PathBuf> {
    let mut packs: Vec<_> = std::fs::read_dir(data_dir().join("packs/personas"))
        .map(|dir| {
            dir.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    packs.sort();
    packs
}

fn daemon_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("LUMEN_DAEMON") {
        return PathBuf::from(bin);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("lumend")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("lumend"))
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("lumen")
}

pub struct LumenApp {
    page: Page,
    socket: PathBuf,
    packs: Vec<PathBuf>,
    pack: usize,
    mode: Mode,
    runtime: Runtime,
    rehearsal: Entity<Room>,
    control: Entity<Studio>,
    preview: Entity<Room>,
    obs: lumen_obs::Controller,
    obs_status: lumen_obs::Status,
    link: Link,
    session: Session,
    editing: Option<usize>,
    apply_on_connect: bool,
    notice: String,
    pickers: Vec<ColorRow>,
    question_input: Entity<InputState>,
    focus: FocusHandle,
    accounts: Vec<crate::accounts::Row>,
    accounts_error: Option<String>,
    confirm_remove: Option<lumen_live::AccountId>,
}

impl LumenApp {
    pub fn new(socket: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_global(Theme::load());
        let packs = packs();
        let pack = packs
            .iter()
            .position(|p| p.file_stem().is_some_and(|s| s == "hakikat-arayisi"))
            .unwrap_or(0);
        let start = match std::env::var("LUMEN_PAGE").as_deref() {
            Ok("control") => Page::Control,
            Ok("appearance") => Page::Look,
            Ok("broadcast") => Page::Broadcast,
            Ok("questions") => Page::Questions,
            Ok("accounts") => Page::Accounts,
            _ => Page::Rehearsal,
        };
        let theme = cx.try_global::<Theme>().cloned().unwrap_or_default();
        let pickers: Vec<_> = palette::Token::ALL
            .into_iter()
            .map(|token| {
                let value: Hsla = theme::color(token.get(&theme)).into();
                let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                let hex = cx.new(|cx| {
                    let mut input = InputState::new(window, cx).placeholder("#rrggbb");
                    input.set_value(token.get(&theme).to_owned(), window, cx);
                    input
                });
                let field = hex.clone();
                cx.subscribe_in(
                    &picker,
                    window,
                    move |app, _, event: &ColorPickerEvent, window, cx| {
                        if let ColorPickerEvent::Change(Some(color)) = event {
                            let value = hsla_hex(*color);
                            field.update(cx, |f, cx| f.set_value(value.clone(), window, cx));
                            app.set_theme(cx, move |t| token.set(t, value));
                        }
                    },
                )
                .detach();
                let swatch = picker.clone();
                cx.subscribe_in(
                    &hex,
                    window,
                    move |app, field, event: &InputEvent, window, cx| {
                        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            return;
                        }
                        let typed = field.read(cx).value().to_string();
                        let current = cx
                            .try_global::<Theme>()
                            .map(|t| token.get(t).to_owned())
                            .unwrap_or_default();
                        match palette::parse_hex(&typed) {
                            Some((r, g, b)) => {
                                let value = palette::to_hex(r, g, b);
                                let color: Hsla = theme::color(&value).into();
                                swatch.update(cx, |p, cx| p.set_value(color, window, cx));
                                field.update(cx, |f, cx| f.set_value(value.clone(), window, cx));
                                if value != current {
                                    app.set_theme(cx, move |t| token.set(t, value));
                                }
                            }
                            None => {
                                if matches!(event, InputEvent::PressEnter { .. }) {
                                    app.notice = format!(
                                        "\"{}\" is not a colour. Use #rrggbb, for example #c9a227.",
                                        typed.trim()
                                    );
                                }
                                field.update(cx, |f, cx| f.set_value(current, window, cx));
                                cx.notify();
                            }
                        }
                    },
                )
                .detach();
                ColorRow { token, picker, hex }
            })
            .collect();
        let question_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Type a question, press Enter to add")
        });
        cx.subscribe_in(
            &question_input,
            window,
            |app, state, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = session::clean(&state.read(cx).value());
                    if !text.is_empty() {
                        let editing = app.editing.take();
                        app.edit_session(cx, |s| match editing {
                            Some(i) if i < s.questions.len() => s.questions[i] = text,
                            _ => s.questions.push(text),
                        });
                    }
                    state.update(cx, |s, cx| s.set_value("", window, cx));
                }
            },
        )
        .detach();
        let mut app = Self {
            page: start,
            rehearsal: cx.new(|cx| {
                let mut room = Room::new(socket.clone(), Layout::Rehearsal, window, cx);
                room.reserved_w = SIDEBAR_W;
                room
            }),
            control: cx.new(|cx| Studio::new(socket.clone(), cx)),
            preview: cx.new(|cx| {
                let mut room = Room::new(socket.clone(), Layout::Stage, window, cx);
                room.max_w = Some(PREVIEW_W - 32.);
                room
            }),
            link: Link::new(socket.clone()),
            socket,
            packs,
            pack,
            mode: Mode::Models,
            obs: lumen_obs::Controller::spawn(),
            obs_status: lumen_obs::Status::default(),
            session: Session::default(),
            editing: None,
            apply_on_connect: false,
            notice: String::new(),
            pickers,
            question_input,
            runtime: Runtime {
                child: None,
                status: String::new(),
            },
            focus: cx.focus_handle(),
            accounts: Vec::new(),
            accounts_error: None,
            confirm_remove: None,
        };
        if start == Page::Accounts {
            app.accounts_op(None, cx);
        }
        app.restart();
        app.publish(cx);
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(500)).await;
                let alive = this.update(cx, |app, cx| {
                    if let Some(status) = app.obs.latest()
                        && status != app.obs_status
                    {
                        app.obs_status = status;
                        app.publish(cx);
                        cx.notify();
                    }
                    app.pump_link(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        app
    }

    fn running_elsewhere(&self) -> bool {
        lumen_protocol::request(
            &self.socket,
            lumen_protocol::Command::Status,
            Duration::from_millis(300),
        )
        .is_ok()
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.runtime.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    /// (Re)starts `lumend` for the selected pack and mode. A runtime started outside the app
    /// (e.g. `lumen yayin`) is reused rather than replaced.
    fn restart(&mut self) {
        self.stop();
        if self.running_elsewhere() {
            self.runtime.status = "Attached to a runtime started outside the app.".into();
            return;
        }
        let Some(pack) = self.packs.get(self.pack).cloned() else {
            self.runtime.status = format!(
                "No pack found in {}",
                data_dir().join("packs/personas").display()
            );
            return;
        };
        self.session = Session::load(&pack);
        self.editing = None;
        self.apply_on_connect = true;
        match self.spawn(&pack) {
            Ok(child) => {
                self.runtime.child = Some(child);
                self.runtime.status = format!("{} ⁜ {}", pack_name(&pack), self.mode.label());
            }
            Err(e) => self.runtime.status = format!("Could not start the runtime: {e}"),
        }
    }

    fn spawn(&self, pack: &Path) -> Result<Child, String> {
        let dir = self.socket.parent().ok_or("invalid socket path")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&self.socket);
        let state = state_dir();
        std::fs::create_dir_all(&state).map_err(|e| e.to_string())?;
        let log = std::fs::File::create(state.join("lumend.log")).map_err(|e| e.to_string())?;
        let mut command = Command::new(daemon_binary());
        match self.mode {
            Mode::Live => {
                command
                    .arg("serve")
                    .arg(&self.socket)
                    .arg(state.join("journal.sqlite"))
                    .arg(pack);
            }
            Mode::Models | Mode::Scripted => {
                command
                    .arg("rehearse")
                    .arg(pack)
                    .arg("--socket")
                    .arg(&self.socket)
                    .arg("--journal")
                    .arg(state.join("rehearsal.sqlite"));
                if self.mode == Mode::Models {
                    command.arg("--models");
                }
            }
        }
        // Live audio goes to Lumen's own sink so OBS captures only the show.
        if self.mode == Mode::Live && lumen_obs::sink_ready() {
            command
                .env("PULSE_SINK", lumen_obs::SINK)
                .env("PIPEWIRE_NODE", lumen_obs::SINK);
        }
        // If the app dies without running its destructors (signal, crash), take lumend with it.
        // SAFETY: prctl is async-signal-safe and touches no Rust state between fork and exec.
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        command
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("{}: {e}", daemon_binary().display()))
    }

    /// Shares the session and OBS status with the other pages (dashboard).
    fn publish(&self, cx: &mut Context<Self>) {
        cx.set_global(Broadcast {
            session: self.session.clone(),
            obs: self.obs_status.clone(),
        });
    }

    /// Applies the saved question list once the runtime answers, and reports results.
    fn pump_link(&mut self, cx: &mut Context<Self>) {
        for (command, result) in self.link.drain() {
            if let lumen_protocol::Command::SetTopics(_) = command {
                match result {
                    Ok(r) if r.error.is_none() => {
                        self.apply_on_connect = false;
                        self.notice = format!(
                            "Questions applied: {} questions, {} in total.",
                            self.session.questions.len(),
                            session::duration(self.session.total_seconds())
                        );
                    }
                    Ok(r) => self.notice = r.error.unwrap_or_default(),
                    Err(_) => {}
                }
                cx.notify();
            }
        }
        if self.apply_on_connect && self.link.idle() {
            self.push_questions();
        }
    }

    fn push_questions(&mut self) {
        match self.session.topic_list() {
            Some(list) => self.link.send(lumen_protocol::Command::SetTopics(list)),
            None => self.notice = "Add at least one question.".into(),
        }
    }

    fn edit_session(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Session)) {
        edit(&mut self.session);
        self.session = std::mem::take(&mut self.session).sanitized();
        if let Some(pack) = self.packs.get(self.pack)
            && let Err(e) = self.session.save(pack)
        {
            self.notice = format!("Could not save questions: {e}");
        }
        self.publish(cx);
        cx.notify();
    }

    fn set_theme(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Theme)) {
        let mut theme = cx.try_global::<Theme>().cloned().unwrap_or_default();
        edit(&mut theme);
        let theme = theme.sanitized();
        if let Err(e) = theme.save() {
            self.runtime.status = format!("Could not save the theme: {e}");
        }
        cx.set_global(theme);
        cx.refresh_windows();
    }
}

impl LumenApp {
    pub fn focus_handle_for_rehearsal(&self, cx: &App) -> FocusHandle {
        self.rehearsal.read(cx).focus.clone()
    }
}

fn pack_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------------------

fn nav_item(id: &'static str, label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h_11()
        .px_3()
        .rounded_lg()
        .text_sm()
        .cursor_pointer()
        .when(active, |d| d.bg(color::muted()))
        .text_color(if active {
            color::foreground()
        } else {
            color::muted_foreground()
        })
        .hover(|s| s.bg(color::muted()))
        .child(label)
}

fn small_label(text: &str) -> Div {
    div()
        .text_xs()
        .text_color(color::subtle_foreground())
        .child(text.to_owned())
}

impl LumenApp {
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let page = self.page;
        let pages = [
            ("nav-rehearsal", "Rehearsal  Ctrl 1", Page::Rehearsal),
            ("nav-questions", "Questions  Ctrl 2", Page::Questions),
            ("nav-control", "Control  Ctrl 3", Page::Control),
            ("nav-look", "Appearance  Ctrl 4", Page::Look),
            ("nav-broadcast", "Broadcast  Ctrl 5", Page::Broadcast),
            ("nav-accounts", "Accounts  Ctrl 6", Page::Accounts),
        ];
        let mut nav = div().flex().flex_col().gap_1();
        for (id, label, target) in pages {
            nav = nav.child(nav_item(id, label, page == target).on_click(cx.listener(
                move |app, _, window, cx| {
                    app.page = target;
                    if target == Page::Accounts {
                        app.accounts_op(None, cx);
                    }
                    let focus = match target {
                        Page::Rehearsal => app.rehearsal.read(cx).focus.clone(),
                        Page::Control => app.control.read(cx).focus.clone(),
                        Page::Look | Page::Broadcast | Page::Questions | Page::Accounts => app.focus.clone(),
                    };
                    window.focus(&focus);
                    cx.notify();
                },
            )));
        }
        let mut pack_list = div().flex().flex_col().gap_1();
        for (index, pack) in self.packs.iter().enumerate() {
            let id = SharedString::from(format!("pack-{index}"));
            pack_list = pack_list.child(
                div()
                    .id(id)
                    .h_9()
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_lg()
                    .text_xs()
                    .cursor_pointer()
                    .when(index == self.pack, |d| {
                        d.border_1().border_color(color::border())
                    })
                    .text_color(if index == self.pack {
                        color::foreground()
                    } else {
                        color::muted_foreground()
                    })
                    .hover(|s| s.bg(color::muted()))
                    .child(pack_name(pack))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if app.pack != index {
                            app.pack = index;
                            app.restart();
                            cx.notify();
                        }
                    })),
            );
        }
        let mut modes = div().flex().flex_col().gap_1();
        for (index, mode) in [Mode::Models, Mode::Scripted, Mode::Live]
            .into_iter()
            .enumerate()
        {
            let id = SharedString::from(format!("mode-{index}"));
            modes = modes.child(
                nav_item("mode", mode.label(), self.mode == mode)
                    .id(id)
                    .h_9()
                    .text_xs()
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if app.mode != mode {
                            app.mode = mode;
                            app.restart();
                            cx.notify();
                        }
                    })),
            );
        }
        let socket = self.socket.clone();
        div()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_6()
            .p_4()
            .border_r_1()
            .border_color(color::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .pt_2()
                    .child(ui::mark(28.))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Lumen"),
                    ),
            )
            .child(nav)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(small_label("Pack"))
                    .child(pack_list),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(small_label("Mode"))
                    .child(modes),
            )
            .child(div().flex_1())
            .child(
                ui::button(
                    "open-frame",
                    "Broadcast frame",
                    "OBS",
                    ui::Variant::Outline,
                    true,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    rehearsal::open(socket.clone(), Layout::Stage, cx);
                })),
            )
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(color::subtle_foreground())
                    .child(self.runtime.status.clone()),
            )
    }
}

// ---------------------------------------------------------------------------------------
// Görünüm (appearance editor)
// ---------------------------------------------------------------------------------------

fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h_9()
        .px_3()
        .flex()
        .items_center()
        .rounded_lg()
        .border_1()
        .border_color(if active {
            color::foreground()
        } else {
            color::border()
        })
        .bg(if active {
            color::foreground()
        } else {
            color::background()
        })
        .text_color(if active {
            color::background()
        } else {
            color::foreground()
        })
        .text_sm()
        .cursor_pointer()
        .child(label.into())
}

fn swatch(id: impl Into<ElementId>, hex: &str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size_9()
        .rounded_lg()
        .border_2()
        .border_color(if active {
            color::foreground()
        } else {
            color::border()
        })
        .bg(theme::color(hex))
        .cursor_pointer()
}

fn row(label: &str) -> Div {
    div().flex().flex_col().gap_2().child(small_label(label))
}

const GROUNDS: [(&str, &str); 6] = [
    ("Black", "#000000"),
    ("Graphite", "#111111"),
    ("Midnight", "#0b1220"),
    ("Burgundy", "#2a0a0f"),
    ("Forest", "#0b1f17"),
    ("Coffee", "#1f140d"),
];

const SIDES: [(&str, &str, &str); 4] = [
    ("Monochrome", "#ffffff", "#ffffff"),
    ("Red ⁜ White", "#e30a17", "#ffffff"),
    ("Teal ⁜ Orange", "#22b8a6", "#f28a3c"),
    ("Blue ⁜ Red", "#3b82f6", "#ef4444"),
];

const QUESTIONS: [(&str, &str); 3] = [
    ("White", "#ffffff"),
    ("Yellow", "#facc15"),
    ("Red", "#e30a17"),
];

const FONTS: [&str; 4] = [
    "Nimbus Sans",
    "Liberation Sans",
    "Adwaita Sans",
    "iA Writer Quattro S",
];

impl LumenApp {
    fn stepper(
        &self,
        id: &'static str,
        label: &str,
        value: String,
        cx: &mut Context<Self>,
        change: fn(&mut Theme, f32),
        step: f32,
    ) -> Div {
        let button = |suffix: &'static str, delta: f32| {
            chip(SharedString::from(format!("{id}-{suffix}")), suffix, false)
                .on_click(cx.listener(move |app, _, _, cx| app.set_theme(cx, |t| change(t, delta))))
        };
        row(label).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(button("−", -step))
                .child(div().w(px(72.)).text_center().text_sm().child(value))
                .child(button("+", step)),
        )
    }

    fn look(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.try_global::<Theme>().cloned().unwrap_or_default();
        let mut modes = div().flex().gap_2();
        for (i, (label, mode)) in [
            ("Solid", Background::Solid),
            ("Gradient", Background::Gradient),
            ("Carousel", Background::Carousel),
        ]
        .into_iter()
        .enumerate()
        {
            modes = modes.child(
                chip(
                    SharedString::from(format!("bg-{i}")),
                    label,
                    theme.background == mode,
                )
                .on_click(
                    cx.listener(move |app, _, _, cx| app.set_theme(cx, |t| t.background = mode)),
                ),
            );
        }
        let swatches = |prefix: &'static str,
                        current: &str,
                        cx: &mut Context<Self>,
                        set: fn(&mut Theme, String)| {
            let mut row = div().flex().flex_wrap().gap_2();
            for (i, (_, hex)) in GROUNDS.iter().enumerate() {
                let hex = (*hex).to_owned();
                row = row.child(
                    swatch(
                        SharedString::from(format!("{prefix}-{i}")),
                        &hex,
                        current == hex,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        let hex = hex.clone();
                        app.set_theme(cx, |t| set(t, hex))
                    })),
                );
            }
            row
        };
        let mut images = div().flex().flex_col().gap_1();
        for (i, image) in theme.images.iter().enumerate() {
            let name = Path::new(image)
                .file_name()
                .map_or_else(|| image.clone(), |n| n.to_string_lossy().into_owned());
            let missing = !theme::expand(image).is_file();
            images = images.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h_9()
                    .px_3()
                    .rounded_lg()
                    .bg(color::muted())
                    .text_xs()
                    .child(if missing {
                        format!("{name} (missing)")
                    } else {
                        name
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("img-del-{i}")))
                            .cursor_pointer()
                            .text_color(color::muted_foreground())
                            .child("Remove")
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.set_theme(cx, |t| {
                                    if i < t.images.len() {
                                        t.images.remove(i);
                                    }
                                })
                            })),
                    ),
            );
        }
        let add =
            |id: &'static str, label: &'static str, directories: bool, cx: &mut Context<Self>| {
                chip(id, label, false).on_click(cx.listener(move |_, _, _, cx| {
                    let picked = cx.prompt_for_paths(PathPromptOptions {
                        files: !directories,
                        directories,
                        multiple: true,
                        prompt: Some("Add".into()),
                    });
                    cx.spawn(async move |app, cx| {
                        let Ok(Ok(Some(paths))) = picked.await else {
                            return;
                        };
                        let mut found = Vec::new();
                        for path in paths {
                            if path.is_dir() {
                                let mut entries: Vec<_> = std::fs::read_dir(&path)
                                    .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
                                    .unwrap_or_default();
                                entries.sort();
                                found.extend(entries.into_iter().filter(|p| is_image(p)));
                            } else if is_image(&path) {
                                found.push(path);
                            }
                        }
                        let _ = app.update(cx, |app, cx| {
                            app.set_theme(cx, |t| {
                                t.background = Background::Carousel;
                                for p in found {
                                    let s = p.display().to_string();
                                    if !t.images.contains(&s) {
                                        t.images.push(s);
                                    }
                                }
                            });
                        });
                    })
                    .detach();
                }))
            };
        let mut sides = div().flex().flex_wrap().gap_2();
        for (i, (label, a, b)) in SIDES.into_iter().enumerate() {
            let active = theme.side_a == a && theme.side_b == b;
            sides = sides.child(
                chip(SharedString::from(format!("side-{i}")), label, active).on_click(cx.listener(
                    move |app, _, _, cx| {
                        app.set_theme(cx, |t| {
                            t.side_a = a.into();
                            t.side_b = b.into();
                        })
                    },
                )),
            );
        }
        let mut questions = div().flex().gap_2();
        for (i, (label, hex)) in QUESTIONS.into_iter().enumerate() {
            questions =
                questions.child(
                    chip(
                        SharedString::from(format!("q-{i}")),
                        label,
                        theme.question == hex,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.set_theme(cx, |t| t.question = hex.into())
                    })),
                );
        }
        let mut fonts = div().flex().flex_wrap().gap_2();
        for (i, font) in FONTS.into_iter().enumerate() {
            fonts = fonts.child(
                chip(
                    SharedString::from(format!("font-{i}")),
                    font,
                    theme.font == font,
                )
                .on_click(
                    cx.listener(move |app, _, _, cx| app.set_theme(cx, |t| t.font = font.into())),
                ),
            );
        }
        let background_card = ui::card(
            "Background",
            Some(
                "What sits behind the broadcast frame. In carousel mode images cross-fade in turn.",
            ),
        )
        .child(modes)
        .child(
            row("Background color")
                .child(swatches("ground", &theme.ground, cx, |t, h| t.ground = h)),
        )
        .when(theme.background == Background::Gradient, |d| {
            d.child(
                row("Gradient end").child(swatches("grad", &theme.gradient_to, cx, |t, h| {
                    t.gradient_to = h
                })),
            )
        })
        .when(theme.background == Background::Carousel, |d| {
            d.child(
                row("Images").child(images).child(
                    div()
                        .flex()
                        .gap_2()
                        .child(add("add-files", "Add images", false, cx))
                        .child(add("add-dir", "Add folder", true, cx)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_6()
                    .child(self.stepper(
                        "secs",
                        "Slide duration",
                        format!("{} s", theme.carousel_seconds),
                        cx,
                        |t, d| t.carousel_seconds = (t.carousel_seconds as f32 + d).max(3.) as u32,
                        2.,
                    ))
                    .child(self.stepper(
                        "dim",
                        "Dimming",
                        format!("{}%", (theme.image_dim * 100.).round()),
                        cx,
                        |t, d| t.image_dim += d,
                        0.05,
                    )),
            )
        });
        let colors_card = self.color_card(&theme);
        let type_card = ui::card("Type and shape", None)
            .child(row("Font").child(fonts))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_6()
                    .child(self.stepper(
                        "topic",
                        "Question",
                        format!("{} px", theme.topic_size),
                        cx,
                        |t, d| t.topic_size += d,
                        1.,
                    ))
                    .child(self.stepper(
                        "caption",
                        "Subtitle",
                        format!("{} px", theme.caption_size),
                        cx,
                        |t, d| t.caption_size += d,
                        1.,
                    ))
                    .child(self.stepper(
                        "name",
                        "Name",
                        format!("{} px", theme.name_size),
                        cx,
                        |t, d| t.name_size += d,
                        1.,
                    ))
                    .child(self.stepper(
                        "rcard",
                        "Card radius",
                        format!("{} px", theme.radius_card),
                        cx,
                        |t, d| t.radius_card += d,
                        2.,
                    ))
                    .child(self.stepper(
                        "rctl",
                        "Label radius",
                        format!("{} px", theme.radius_control),
                        cx,
                        |t, d| t.radius_control += d,
                        2.,
                    )),
            );
        let reset = ui::card(
            "Default",
            Some("onur.af design language: monochrome, Helvetica, 12 / 8 px corners, oriental carousel."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    chip("reset", "Reset to default", false).on_click(cx.listener(
                        |app, _, window, cx| {
                            app.set_theme(cx, |t| *t = Theme::default());
                            app.sync_pickers(window, cx);
                        },
                    )),
                )
                .child(small_label(
                    &theme::path().map_or_else(String::new, |p| p.display().to_string()),
                )),
        );
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("look-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .p_6()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Appearance"),
                            )
                            .child(background_card)
                            .child(colors_card)
                            .child(type_card)
                            .child(reset),
                    ),
            )
            .child(
                div()
                    .w(relative(0.42))
                    .max_w(px(PREVIEW_W))
                    .h_full()
                    .flex_none()
                    .p_4()
                    .border_l_1()
                    .border_color(color::border())
                    .child(self.preview.clone()),
            )
    }
}

// ---------------------------------------------------------------------------------------
// Yayın (OBS)
// ---------------------------------------------------------------------------------------

fn hms(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

impl LumenApp {
    fn step(
        &self,
        id: &'static str,
        title: &str,
        detail: &str,
        ok: bool,
        action: Option<(&'static str, lumen_obs::Action)>,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .min_h(px(56.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(ui::badge(
                        if ok { "Ready" } else { "Waiting" },
                        if ok { Tone::Success } else { Tone::Neutral },
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().child(title.to_owned()))
                            .child(small_label(detail)),
                    ),
            )
            .children(action.map(|(label, action)| {
                chip(id, label, false).on_click(cx.listener(move |app, _, _, _| {
                    app.obs.send(action.clone());
                }))
            }))
    }

    fn broadcast(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let s = &self.obs_status;
        let url = lumen_obs::overlay_url(overlay_port());
        let setup = ui::card(
            "Kurulum",
            Some("Lumen builds its own scene collection in OBS and never touches your scenes."),
        )
        .child(self.step(
            "ws",
            "OBS WebSocket",
            if s.websocket_enabled {
                "On"
            } else {
                "Off. OBS must be closed to turn it on."
            },
            s.websocket_enabled,
            (!s.websocket_enabled).then_some(("Turn on", lumen_obs::Action::EnableWebsocket)),
            cx,
        ))
        .child(self.step(
            "obs",
            "OBS Studio",
            if s.obs_running { "Running" } else { "Off" },
            s.obs_running,
            (!s.obs_running).then_some(("Start OBS", lumen_obs::Action::LaunchObs)),
            cx,
        ))
        .child(self.step(
            "conn",
            "Connection",
            if s.connected {
                "Lumen is connected to OBS"
            } else {
                "Not connected"
            },
            s.connected,
            None,
            cx,
        ))
        .child(self.step(
            "scene",
            "Lumen scene",
            "1080×1920 ⁜ 30 fps ⁜ broadcast frame + Lumen audio",
            s.collection_ready,
            s.connected.then_some((
                if s.collection_ready {
                    "Rebuild"
                } else {
                    "Build scene"
                },
                lumen_obs::Action::Setup {
                    overlay: url.clone(),
                    socket: self.socket.clone(),
                },
            )),
            cx,
        ))
        .child(self.step(
            "audio",
            "Audio line (PipeWire)",
            if s.sink_ready {
                "The Lumen-Yayin virtual sink is ready; only show audio reaches the stream."
            } else {
                "Created when the scene is built."
            },
            s.sink_ready,
            s.sink_ready.then_some((
                if s.monitoring {
                    "Stop monitoring"
                } else {
                    "Monitor in headphones"
                },
                lumen_obs::Action::Monitor(!s.monitoring),
            )),
            cx,
        ));
        let live = s.streaming;
        let output = ui::card(
            "Broadcast",
            Some("The stream server and key live in OBS ⁜ Settings ⁜ Stream; enter the key there yourself."),
        )
        .child(
            div()
                .flex()
                .gap_3()
                .child(
                    ui::button(
                        "stream",
                        if live { "Stop stream" } else { "Start stream" },
                        if live { hms(s.stream_seconds) } else { "OBS".into() }.as_str(),
                        if live { ui::Variant::Destructive } else { ui::Variant::Active },
                        s.connected,
                    )
                    .flex_1()
                    .on_click(cx.listener(move |app, _, _, _| {
                        app.obs.send(if live {
                            lumen_obs::Action::StopStream
                        } else {
                            lumen_obs::Action::StartStream
                        })
                    })),
                )
                .child({
                    let recording = s.recording;
                    ui::button(
                        "record",
                        if recording { "Stop recording" } else { "Start recording" },
                        "REC",
                        ui::Variant::Outline,
                        s.connected,
                    )
                    .flex_1()
                    .on_click(cx.listener(move |app, _, _, _| {
                        app.obs.send(if recording {
                            lumen_obs::Action::StopRecord
                        } else {
                            lumen_obs::Action::StartRecord
                        })
                    }))
                }),
        )
        .child(
            div()
                .flex()
                .gap_3()
                .child(ui::stat("Status", if live { "Live" } else { "Off" }, if live { Tone::Danger } else { Tone::Neutral }))
                .child(ui::stat("Congestion", format!("{:.0}%", s.congestion * 100.), if s.congestion > 0.5 { Tone::Warning } else { Tone::Success }))
                .child(ui::stat(
                    "Skipped frames",
                    format!("{} / {}", s.skipped_frames, s.total_frames),
                    if s.skipped_frames > 0 { Tone::Warning } else { Tone::Success },
                )),
        );
        let overlay = ui::card(
            "Broadcast frame",
            Some("OBS builds with Browser Source load this page (1080×1920); otherwise Lumen drives native OBS sources."),
        )
        .child(div().font_family(ui::MONO).text_sm().child(url.clone()));
        div()
            .id("broadcast-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .max_w(px(760.))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Broadcast"),
                    )
                    .when(!s.message.is_empty(), |d| {
                        d.child(
                            div()
                                .text_sm()
                                .text_color(color::muted_foreground())
                                .child(s.message.clone()),
                        )
                    })
                    .child(setup)
                    .child(output)
                    .child(overlay),
            )
    }
}

// ---------------------------------------------------------------------------------------
// Accounts (Account Center)
// ---------------------------------------------------------------------------------------

impl LumenApp {
    /// Apply an account operation off the UI thread, then refresh the list.
    fn accounts_op(&mut self, op: Option<crate::accounts::Op>, cx: &mut Context<Self>) {
        let home = lumen_accounts::default_home();
        let task = cx.background_executor().spawn(async move { crate::accounts::run(&home, op) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |app, cx| {
                match result {
                    Ok(rows) => {
                        app.accounts = rows;
                        app.accounts_error = None;
                    }
                    Err(e) => app.accounts_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn accounts_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use crate::accounts::Op;
        let mut list = div().flex().flex_col().gap_2();
        for (i, a) in self.accounts.iter().enumerate() {
            let id = a.id.clone();
            let state = a.state.clone().unwrap_or_else(|| "not connected yet".into());
            let tone = match state.as_str() {
                "live" => Tone::Success,
                "failed" => Tone::Danger,
                "backoff" | "offline" => Tone::Warning,
                _ => Tone::Neutral,
            };
            let confirming = self.confirm_remove.as_ref() == Some(&a.id);
            let (live_id, bc_id, rm_id, use_id) = (id.clone(), id.clone(), id.clone(), id.clone());
            let (live, broadcast) = (a.live, a.broadcast);
            let row = div()
                .id(SharedString::from(format!("acct-row-{i}")))
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .rounded_lg()
                .border_1()
                .border_color(if a.active { color::foreground() } else { color::border() })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().font_weight(FontWeight::MEDIUM).child(a.label.clone()))
                        .child(div().text_sm().font_family(ui::MONO).text_color(color::muted_foreground()).child(format!("{} @{}", a.platform, a.handle)))
                        .children(a.group.clone().map(|g| ui::badge(g, Tone::Info)))
                        .children(a.active.then(|| ui::badge("active", Tone::Success)))
                        .children((!a.enabled).then(|| ui::badge("disabled", Tone::Neutral)))
                        .child(div().flex_1())
                        .child(ui::badge(match a.events { Some(n) => format!("{state} · {n} events"), None => state.clone() }, tone)),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(chip(SharedString::from(format!("acct-use-{i}")), "Make active", a.active).on_click(cx.listener(move |app, _, _, cx| app.accounts_op(Some(Op::Use(use_id.clone())), cx))))
                        .child(chip(SharedString::from(format!("acct-live-{i}")), if live { "LIVE events on" } else { "LIVE events off" }, live).on_click(cx.listener(move |app, _, _, cx| app.accounts_op(Some(Op::Live(live_id.clone(), !live)), cx))))
                        .child(if a.authorized {
                            chip(SharedString::from(format!("acct-bc-{i}")), if broadcast { "Broadcast on" } else { "Broadcast off" }, broadcast).on_click(cx.listener(move |app, _, _, cx| app.accounts_op(Some(Op::Broadcast(bc_id.clone(), !broadcast)), cx)))
                        } else {
                            div().id(SharedString::from(format!("acct-bc-{i}"))).text_xs().text_color(color::muted_foreground()).child("Broadcast: not authorized")
                        })
                        .child(div().flex_1())
                        .child(chip(SharedString::from(format!("acct-rm-{i}")), if confirming { "Confirm remove" } else { "Remove" }, confirming).on_click(cx.listener(move |app, _, _, cx| {
                            if app.confirm_remove.as_ref() == Some(&rm_id) {
                                app.confirm_remove = None;
                                app.accounts_op(Some(Op::Remove(rm_id.clone())), cx);
                            } else {
                                app.confirm_remove = Some(rm_id.clone());
                                cx.notify();
                            }
                        }))),
                );
            list = list.child(row);
        }
        let body = ui::card(
            "Account Center",
            Some("TikTok, Kwai Global and Likee Global accounts. Add accounts, secrets and broadcast authorization with `lumen-live accounts`; run them with `lumen-live connect`."),
        )
        .children(self.accounts_error.clone().map(|e| div().text_sm().text_color(color::danger()).child(e)))
        .children(self.accounts.is_empty().then(|| div().text_sm().text_color(color::muted_foreground()).child("No accounts yet: lumen-live accounts add --platform kwai --label NAME --handle HANDLE")))
        .child(list)
        .child(chip("acct-refresh", "Refresh", false).on_click(cx.listener(|app, _, _, cx| app.accounts_op(None, cx))));
        div()
            .id("accounts-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(div().p_6().flex().flex_col().gap_5().max_w(px(900.)).child(div().text_xl().font_weight(FontWeight::MEDIUM).child("Accounts")).child(body))
    }
}

// ---------------------------------------------------------------------------------------
// Sorular (session questions)
// ---------------------------------------------------------------------------------------

impl LumenApp {
    fn questions(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let s = &self.session;
        let total = session::duration(s.total_seconds());
        let summary = format!(
            "{} questions × {} min = {} on air",
            s.questions.len(),
            s.minutes,
            total
        );
        let current = cx
            .try_global::<Broadcast>()
            .map(|_| ())
            .and(self.rehearsal.read(cx).current_topic());
        let editing = self.editing;
        let mut list = div().flex().flex_col().gap_2();
        for (i, question) in s.questions.iter().enumerate() {
            let live = current == Some(i + 1);
            let row = div()
                .id(SharedString::from(format!("q-row-{i}")))
                .flex()
                .items_center()
                .gap_3()
                .min_h(px(52.))
                .px_4()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(if editing == Some(i) || live {
                    color::foreground()
                } else {
                    color::border()
                })
                .bg(if live {
                    color::muted()
                } else {
                    color::background()
                })
                .child(
                    div()
                        .w(px(28.))
                        .flex_none()
                        .text_sm()
                        .font_family(ui::MONO)
                        .text_color(color::muted_foreground())
                        .child(format!("{:02}", i + 1)),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("q-text-{i}")))
                        .debug_selector(|| format!("question-{i}"))
                        .flex_1()
                        .min_w_0()
                        .text_base()
                        .cursor_pointer()
                        .child(question.clone())
                        .on_click(cx.listener(move |app, _, window, cx| {
                            let text = app.session.questions.get(i).cloned().unwrap_or_default();
                            app.question_input
                                .update(cx, |s, cx| s.set_value(text, window, cx));
                            app.question_input.read(cx).focus_handle(cx).focus(window);
                            app.editing = Some(i);
                            cx.notify();
                        })),
                )
                .when(live, |d| d.child(ui::badge("On air", Tone::Success)))
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            chip(SharedString::from(format!("q-up-{i}")), "↑", false).on_click(
                                cx.listener(move |app, _, _, cx| {
                                    app.edit_session(cx, |s| {
                                        if i > 0 {
                                            s.questions.swap(i, i - 1)
                                        }
                                    })
                                }),
                            ),
                        )
                        .child(
                            chip(SharedString::from(format!("q-down-{i}")), "↓", false).on_click(
                                cx.listener(move |app, _, _, cx| {
                                    app.edit_session(cx, |s| {
                                        if i + 1 < s.questions.len() {
                                            s.questions.swap(i, i + 1)
                                        }
                                    })
                                }),
                            ),
                        )
                        .child(
                            chip(SharedString::from(format!("q-del-{i}")), "Delete", false)
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.editing = None;
                                    app.edit_session(cx, |s| {
                                        if i < s.questions.len() {
                                            s.questions.remove(i);
                                        }
                                    })
                                })),
                        ),
                );
            list = list.child(row);
        }
        let input = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .debug_selector(|| "question-input".into())
                    .child(Input::new(&self.question_input).cleanable(true)),
            )
            .child(small_label(if editing.is_some() {
                "Editing an existing question: press Enter to save, or clear the field to add a new one."
            } else {
                "Press Enter to add. Click a question to edit it."
            }));
        let minutes = s.minutes;
        div()
            .id("questions-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .max_w(px(900.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_xl().font_weight(FontWeight::MEDIUM).child("Questions"))
                                    .child(small_label(&summary)),
                            )
                            .child(
                                ui::button("apply-questions", "Apply to broadcast", "↵", ui::Variant::Active, true)
                                    .w(px(220.))
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.push_questions();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(!self.notice.is_empty(), |d| {
                        d.child(div().text_sm().text_color(color::muted_foreground()).child(self.notice.clone()))
                    })
                    .child(
                        ui::card(
                            "Session",
                            Some("Each question is its own round; when time is up the next one starts. After the last question the session ends."),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(small_label("Minutes per question"))
                                .child(chip("min-down", "−", false).on_click(cx.listener(|app, _, _, cx| {
                                    app.edit_session(cx, |s| s.minutes = s.minutes.saturating_sub(1).max(1))
                                })))
                                .child(div().w(px(64.)).text_center().text_sm().child(format!("{minutes} min")))
                                .child(chip("min-up", "+", false).on_click(cx.listener(|app, _, _, cx| {
                                    app.edit_session(cx, |s| s.minutes = (s.minutes + 1).min(180))
                                })))
                                .child(div().flex_1())
                                .child(div().text_sm().child(format!("Total on air: {total}"))),
                        ),
                    )
                    .child(input)
                    .child(list)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(small_label("Applying restarts the session from the first question."))
                            .child(div().flex_1())
                            .child(chip("reset-questions", "Restore pack questions", false).on_click(
                                cx.listener(|app, _, _, cx| {
                                    if let Some(pack) = app.packs.get(app.pack).cloned() {
                                        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME")
                                            .map(PathBuf::from)
                                            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                                        {
                                            let _ = std::fs::remove_file(dir.join("lumen/sessions").join(format!("{}.toml", pack_name(&pack))));
                                        }
                                        app.session = Session::load(&pack);
                                        app.editing = None;
                                        app.publish(cx);
                                        cx.notify();
                                    }
                                }),
                            )),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------------------
// Colours (gpui-component ColorPicker per theme token)
// ---------------------------------------------------------------------------------------

fn luminance(hex: &str) -> f32 {
    let (r, g, b) = palette::parse_hex(hex).unwrap_or((255, 255, 255));
    let lin = |c: u8| {
        let c = f32::from(c) / 255.;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio between two colours (1..21).
fn contrast(a: &str, b: &str) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn hsla_hex(color: Hsla) -> String {
    let c: Rgba = color.into();
    let to = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    palette::to_hex(to(c.r), to(c.g), to(c.b))
}

/// One editable theme colour: the kit picker (swatch, popover, palettes) and a hex field.
struct ColorRow {
    token: palette::Token,
    picker: Entity<ColorPickerState>,
    hex: Entity<InputState>,
}

impl LumenApp {
    /// Re-syncs every picker with the theme (after a reset or an external change).
    fn sync_pickers(&self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = cx.try_global::<Theme>().cloned().unwrap_or_default();
        for row in &self.pickers {
            let hex = row.token.get(&theme).to_owned();
            let value: Hsla = theme::color(&hex).into();
            row.picker
                .update(cx, |s, cx| s.set_value(value, window, cx));
            row.hex.update(cx, |s, cx| s.set_value(hex, window, cx));
        }
    }

    fn color_card(&self, theme: &Theme) -> Div {
        let featured: Vec<Hsla> = palette::FEATURED
            .iter()
            .map(|h| theme::color(h).into())
            .collect();
        let mut rows = div().flex().flex_col().gap_1();
        for (i, row) in self.pickers.iter().enumerate() {
            let token = row.token;
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_h(px(52.))
                    .px_2()
                    .rounded_lg()
                    .hover(|s| s.bg(color::muted()))
                    .child(
                        div()
                            .flex_none()
                            .debug_selector(|| format!("picker-{i}"))
                            .child(ColorPicker::new(&row.picker).featured_colors(featured.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_sm()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(token.label()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(color::subtle_foreground())
                                    .truncate()
                                    .child(token.hint()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(104.))
                            .font_family(ui::MONO)
                            .debug_selector(|| format!("hex-{i}"))
                            .child(Input::new(&row.hex).small()),
                    ),
            );
        }
        let text_on_ground = contrast(&theme.text, &theme.ground);
        let text_on_panel = contrast(&theme.text, &theme.surface);
        let readable = text_on_ground.min(text_on_panel);
        let (verdict, tone) = if readable >= 7. {
            ("Excellent contrast", Tone::Success)
        } else if readable >= 4.5 {
            ("Readable", Tone::Success)
        } else if readable >= 3. {
            ("Large text only", Tone::Warning)
        } else {
            ("Hard to read", Tone::Danger)
        };
        ui::card(
            "Colors",
            Some("Pick any color: every shade of every family, your own hex, or the featured Lumen tones. Changes go to the broadcast frame instantly."),
        )
        .child(rows)
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(ui::badge(verdict, tone))
                .child(small_label(&format!(
                    "Text on background {text_on_ground:.1}:1 ⁜ text on panel {text_on_panel:.1}:1 (WCAG AA needs 4.5:1)"
                ))),
        )
    }
}

fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
    })
}

impl Render for LumenApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.page {
            Page::Rehearsal => self.rehearsal.clone().into_any_element(),
            Page::Control => self.control.clone().into_any_element(),
            Page::Look => self.look(cx).into_any_element(),
            Page::Broadcast => self.broadcast(cx).into_any_element(),
            Page::Questions => self.questions(cx).into_any_element(),
            Page::Accounts => self.accounts_page(cx).into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                let k = &event.keystroke;
                if !k.modifiers.control {
                    return;
                }
                let target = match k.key.as_str() {
                    "1" => Page::Rehearsal,
                    "2" => Page::Questions,
                    "3" => Page::Control,
                    "4" => Page::Look,
                    "5" => Page::Broadcast,
                    "6" => Page::Accounts,
                    _ => return,
                };
                app.page = target;
                if target == Page::Accounts {
                    app.accounts_op(None, cx);
                }
                let focus = match target {
                    Page::Rehearsal => app.rehearsal.read(cx).focus.clone(),
                    Page::Control => app.control.read(cx).focus.clone(),
                    Page::Look | Page::Broadcast | Page::Questions | Page::Accounts => app.focus.clone(),
                };
                window.focus(&focus);
                cx.stop_propagation();
                cx.notify();
            }))
            .bg(color::background())
            .font_family(ui::FONT)
            .text_color(color::foreground())
            .child(self.sidebar(cx))
            .child(div().flex_1().h_full().overflow_hidden().child(content))
    }
}

/// Headless interaction tests: real clicks and keystrokes through GPUI's test platform.
#[cfg(test)]
mod interaction {
    use super::*;
    use std::{cell::RefCell, rc::Rc, sync::Mutex};
    // `use gpui::*` above exports gpui::test, which would shadow the built-in `#[test]`.
    use core::prelude::v1::test;

    /// The app reads its config from the environment, so tests take turns.
    static ENV: Mutex<()> = Mutex::new(());

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // SAFETY: tests touching the environment are serialized by `ENV`.
        unsafe {
            for key in [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_RUNTIME_DIR",
            ] {
                std::env::set_var(key, &dir);
            }
            std::env::set_var("LUMEN_HOME", repo);
            std::env::set_var("LUMEN_DAEMON", "/bin/true");
            std::env::remove_var("LUMEN_PAGE");
        }
        dir
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
        page: Page,
    ) -> (Entity<LumenApp>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::kit_theme(cx);
        });
        let slot = Rc::new(RefCell::new(None));
        let socket = dir.join("control.sock");
        let (_, vcx) = cx.add_window_view({
            let slot = slot.clone();
            move |window, cx| {
                let app = cx.new(|cx| LumenApp::new(socket, window, cx));
                *slot.borrow_mut() = Some(app.clone());
                gpui_component::Root::new(app, window, cx)
            }
        });
        let app = slot.borrow_mut().take().unwrap();
        app.update(vcx, |app, cx| {
            app.page = page;
            cx.notify();
        });
        vcx.run_until_parked();
        (app, vcx)
    }

    fn click(vcx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        let bounds = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not rendered"));
        vcx.simulate_click(bounds.center(), Modifiers::none());
        vcx.run_until_parked();
        bounds
    }

    fn saved_theme(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("lumen/theme.toml")).unwrap_or_default()
    }

    #[gpui::test]
    fn picker_popover_swatch_and_hex_field_recolour_the_frame(cx: &mut TestAppContext) {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = sandbox("color");
        let (app, vcx) = open(cx, &dir, Page::Look);
        // Tall enough that the popover opens below the swatch instead of snapping to the edge.
        vcx.simulate_resize(size(px(1440.), px(1600.)));
        vcx.run_until_parked();
        let side_a = palette::Token::ALL
            .iter()
            .position(|t| *t == palette::Token::SideA)
            .unwrap();
        let theme_hex = |vcx: &mut VisualTestContext| {
            vcx.update(|_, cx| palette::Token::SideA.get(cx.global::<Theme>()).to_owned())
        };
        assert_eq!(theme_hex(vcx), "#ffffff");

        // Swatch click opens the kit popover; a featured swatch sits at the popover's top-left.
        let selector = [
            "picker-0", "picker-1", "picker-2", "picker-3", "picker-4", "picker-5", "picker-6",
            "picker-7", "picker-8",
        ][side_a];
        let swatch = click(vcx, selector);
        let row = app.read_with(vcx, |app, _| app.pickers[side_a].picker.clone());
        // Featured item 3 (#8b1e2d): popover at (left, bottom + 6), padding 12, 20px items, 4px gap.
        let item = point(
            swatch.left() + px(12. + 3. * 24. + 10.),
            swatch.bottom() + px(6. + 12. + 10.),
        );
        vcx.simulate_click(item, Modifiers::none());
        vcx.run_until_parked();
        assert_eq!(
            theme_hex(vcx),
            "#8b1e2d",
            "featured swatch in the popover applies"
        );
        assert!(saved_theme(&dir).contains("#8b1e2d"), "theme.toml is saved");
        let field = app.read_with(vcx, |app, _| app.pickers[side_a].hex.clone());
        assert_eq!(
            field.read_with(vcx, |f, _| f.value().to_string()),
            "#8b1e2d"
        );

        // Any colour by hex: click the field, replace its text, Enter.
        let hex = [
            "hex-0", "hex-1", "hex-2", "hex-3", "hex-4", "hex-5", "hex-6", "hex-7", "hex-8",
        ][side_a];
        click(vcx, hex);
        vcx.simulate_keystrokes("ctrl-a backspace");
        vcx.simulate_input("F80");
        vcx.simulate_keystrokes("enter");
        assert_eq!(
            theme_hex(vcx),
            "#ff8800",
            "short hex is expanded and applied"
        );
        assert_eq!(
            field.read_with(vcx, |f, _| f.value().to_string()),
            "#ff8800"
        );
        let picked = row.read_with(vcx, |p, _| p.value().map(hsla_hex));
        assert_eq!(
            picked.as_deref(),
            Some("#ff8800"),
            "swatch follows the field"
        );

        // Garbage is refused and the field falls back to the current colour.
        vcx.simulate_keystrokes("ctrl-a backspace");
        vcx.simulate_input("nope");
        vcx.simulate_keystrokes("enter");
        assert_eq!(theme_hex(vcx), "#ff8800");
        assert_eq!(
            field.read_with(vcx, |f, _| f.value().to_string()),
            "#ff8800"
        );
        assert!(app.read_with(vcx, |app, _| app.notice.contains("not a colour")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui::test]
    fn questions_are_typed_added_and_edited_in_place(cx: &mut TestAppContext) {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = sandbox("questions");
        let (app, vcx) = open(cx, &dir, Page::Questions);
        let before = app.read_with(vcx, |app, _| app.session.questions.len());
        assert!(before > 0, "the pack seeds the session");

        click(vcx, "question-input");
        vcx.simulate_input("Cumhuriyet kime kazandırdı?");
        vcx.simulate_keystrokes("enter");
        let questions = app.read_with(vcx, |app, _| app.session.questions.clone());
        assert_eq!(questions.len(), before + 1);
        assert_eq!(questions.last().unwrap(), "Cumhuriyet kime kazandırdı?");
        let input = app.read_with(vcx, |app, _| app.question_input.clone());
        assert_eq!(
            input.read_with(vcx, |i, _| i.value().to_string()),
            "",
            "input clears"
        );
        let saved = std::fs::read_to_string(dir.join("lumen/sessions/hakikat-arayisi.toml"))
            .expect("session saved");
        assert!(saved.contains("Cumhuriyet kime kazandırdı?"));

        // Click a question: it loads into the input; Enter replaces it instead of appending.
        click(vcx, "question-0");
        let first = app.read_with(vcx, |app, _| app.session.questions[0].clone());
        assert_eq!(input.read_with(vcx, |i, _| i.value().to_string()), first);
        assert_eq!(app.read_with(vcx, |app, _| app.editing), Some(0));
        vcx.simulate_keystrokes("ctrl-a backspace");
        vcx.simulate_input("Laiklik kimi korur?");
        vcx.simulate_keystrokes("enter");
        let questions = app.read_with(vcx, |app, _| app.session.questions.clone());
        assert_eq!(questions.len(), before + 1, "edit does not append");
        assert_eq!(questions[0], "Laiklik kimi korur?");
        assert_eq!(app.read_with(vcx, |app, _| app.editing), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
