//! OBS Studio control for Lumen.
//!
//! - **WebSocket:** connection details come from OBS's own `obs-websocket` config at runtime;
//!   the password is never logged or shown. `enable_websocket` turns the server on while OBS
//!   is closed, keeping the user's password.
//! - **Scene:** a dedicated "Lumen" scene collection (the user's collections stay untouched),
//!   1080x1920 at 30 fps, scene "Debate" with the Lumen overlay as a Browser Source and the
//!   Lumen audio sink as an output capture; global desktop/mic audio is muted there so other
//!   apps never leak into the stream. Setup is idempotent.
//! - **Audio:** a PipeWire/Pulse null sink "Lumen Broadcast" that `lumend` plays into, plus an
//!   optional loopback so the operator can hear it.
//! - **Output:** start/stop stream and recording, and status. The stream service (TikTok RTMP
//!   URL and key) stays in OBS's own settings; Lumen never handles the stream key.
pub mod native;

use obws::{Client, requests::config::SetVideoSettings, requests::inputs::Create};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub const COLLECTION: &str = "Lumen";
pub const SCENE: &str = "Debate";
pub const OVERLAY_INPUT: &str = "Lumen broadcast frame";
pub const AUDIO_INPUT: &str = "Lumen audio";
pub const SINK: &str = "lumen_broadcast";
pub const SINK_DESCRIPTION: &str = "Lumen-Yayin";
pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;

// ---------------------------------------------------------------------------------------
// obs-websocket configuration
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsocketConfig {
    pub enabled: bool,
    pub port: u16,
    pub password: Option<String>,
}

pub fn websocket_config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("obs-studio/plugin_config/obs-websocket/config.json"))
}

pub fn read_websocket_config() -> Option<WebsocketConfig> {
    let text = std::fs::read_to_string(websocket_config_path()?).ok()?;
    parse_websocket_config(&text)
}

pub fn parse_websocket_config(text: &str) -> Option<WebsocketConfig> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let auth = json
        .get("auth_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(WebsocketConfig {
        enabled: json
            .get("server_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        port: json
            .get("server_port")
            .and_then(|v| v.as_u64())
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(4455),
        password: auth
            .then(|| {
                json.get("server_password")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .flatten()
            .filter(|p| !p.is_empty()),
    })
}

pub fn obs_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "obs"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Turns the WebSocket server on in OBS's config. OBS rewrites its config on exit, so this
/// only works while OBS is closed. Other keys (including the password) are preserved.
pub fn enable_websocket() -> Result<(), String> {
    if obs_running() {
        return Err("OBS is running; close it first to change this setting.".into());
    }
    let path = websocket_config_path().ok_or("config directory not found")?;
    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({ "server_port": 4455, "auth_required": true }));
    json["server_enabled"] = serde_json::Value::Bool(true);
    json["first_load"] = serde_json::Value::Bool(false);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn launch_obs() -> Result<(), String> {
    if obs_running() {
        return Ok(());
    }
    Command::new("obs")
        .args(["--minimize-to-tray", "--disable-shutdown-check"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start OBS: {e}"))
}

// ---------------------------------------------------------------------------------------
// PipeWire / Pulse audio sink
// ---------------------------------------------------------------------------------------

fn pactl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|e| format!("pactl: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

pub fn sink_ready() -> bool {
    pactl(&["list", "short", "sinks"])
        .is_ok_and(|s| s.lines().any(|l| l.split_whitespace().nth(1) == Some(SINK)))
}

/// Creates the "Lumen Broadcast" null sink if missing (runtime only; recreated on next launch).
pub fn ensure_sink() -> Result<(), String> {
    if sink_ready() {
        return Ok(());
    }
    pactl(&[
        "load-module",
        "module-null-sink",
        &format!("sink_name={SINK}"),
        &format!("sink_properties=device.description={SINK_DESCRIPTION}"),
    ])
    .map(|_| ())
}

fn loopback_ids() -> Vec<String> {
    pactl(&["list", "short", "modules"])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("module-loopback") && l.contains(&format!("{SINK}.monitor")))
        .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
        .collect()
}

pub fn monitoring() -> bool {
    !loopback_ids().is_empty()
}

/// Lets the operator hear the broadcast sink on their default output.
pub fn set_monitoring(on: bool) -> Result<(), String> {
    if on {
        if monitoring() {
            return Ok(());
        }
        ensure_sink()?;
        pactl(&[
            "load-module",
            "module-loopback",
            &format!("source={SINK}.monitor"),
            "latency_msec=40",
        ])
        .map(|_| ())
    } else {
        for id in loopback_ids() {
            pactl(&["unload-module", &id])?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Scene setup and outputs (async, obws)
// ---------------------------------------------------------------------------------------

pub fn overlay_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

pub fn overlay_settings(url: &str) -> serde_json::Value {
    serde_json::json!({
        "url": url,
        "width": WIDTH,
        "height": HEIGHT,
        "fps": 30,
        "fps_custom": true,
        "reroute_audio": false,
        "shutdown": false,
        "restart_when_active": false,
        "css": "body { background-color: rgba(0, 0, 0, 0); margin: 0; overflow: hidden; }",
    })
}

pub fn audio_settings() -> serde_json::Value {
    serde_json::json!({ "device_id": SINK })
}

pub async fn connect(config: &WebsocketConfig) -> Result<Client, String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        Client::connect("127.0.0.1", config.port, config.password.as_deref()),
    )
    .await
    .map_err(|_| "Could not connect to OBS (timeout)".to_string())?
    .map_err(|e| format!("Could not connect to OBS: {e}"))
}

/// How the frame reaches OBS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// HTML overlay as a Browser Source (OBS builds with obs-browser).
    Browser,
    /// Native OBS sources driven live by Lumen.
    Native,
}

pub async fn setup(
    client: &Client,
    overlay: &str,
    avatars: &[String; 2],
) -> Result<(String, Option<native::Driver>), String> {
    let e = |what: &str| {
        let what = what.to_owned();
        move |err: obws::error::Error| format!("{what}: {err}")
    };
    let collections = client
        .scene_collections()
        .list()
        .await
        .map_err(e("scene collections"))?;
    if !collections.collections.iter().any(|c| c == COLLECTION) {
        client
            .scene_collections()
            .create(COLLECTION)
            .await
            .map_err(e("create collection"))?;
    } else if collections.current != COLLECTION {
        client
            .scene_collections()
            .set_current(COLLECTION)
            .await
            .map_err(e("select collection"))?;
    }
    // Collection switches are asynchronous in OBS; give it a moment.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let streaming = client
        .streaming()
        .status()
        .await
        .map(|s| s.active)
        .unwrap_or(false);
    let recording = client
        .recording()
        .status()
        .await
        .map(|s| s.active)
        .unwrap_or(false);
    if !streaming && !recording {
        client
            .config()
            .set_video_settings(SetVideoSettings {
                fps_numerator: Some(30),
                fps_denominator: Some(1),
                base_width: Some(WIDTH),
                base_height: Some(HEIGHT),
                output_width: Some(WIDTH),
                output_height: Some(HEIGHT),
            })
            .await
            .map_err(e("video settings"))?;
    }
    let scenes = client.scenes().list().await.map_err(e("sahneler"))?;
    if !scenes.scenes.iter().any(|s| s.id.name == SCENE) {
        client
            .scenes()
            .create(SCENE)
            .await
            .map_err(e("create scene"))?;
    }
    client
        .scenes()
        .set_current_program_scene(SCENE)
        .await
        .map_err(e("select scene"))?;
    let kinds = client
        .inputs()
        .list_kinds(false)
        .await
        .map_err(e("input kinds"))?;
    let composition = if kinds.iter().any(|k| k == "browser_source") {
        Composition::Browser
    } else {
        Composition::Native
    };
    let mut driver = None;
    if composition == Composition::Browser {
        let inputs = client.inputs().list(None).await.map_err(e("inputs"))?;
        if inputs.iter().any(|i| i.id.name == OVERLAY_INPUT) {
            client
                .inputs()
                .set_settings(obws::requests::inputs::SetSettings {
                    input: OVERLAY_INPUT.into(),
                    settings: &overlay_settings(overlay),
                    overlay: Some(true),
                })
                .await
                .map_err(e("frame settings"))?;
        } else {
            client
                .inputs()
                .create(Create {
                    scene: SCENE.into(),
                    input: OVERLAY_INPUT,
                    kind: "browser_source",
                    settings: Some(overlay_settings(overlay)),
                    enabled: Some(true),
                })
                .await
                .map_err(e("frame source"))?;
        }
    } else {
        driver = Some(native::build(client, SCENE, &native::Look::load(), avatars).await?);
    }
    let inputs = client.inputs().list(None).await.map_err(e("inputs"))?;
    let exists = |name: &str| inputs.iter().any(|i| i.id.name == name);
    if !exists(AUDIO_INPUT) {
        client
            .inputs()
            .create(Create {
                scene: SCENE.into(),
                input: AUDIO_INPUT,
                kind: "pulse_output_capture",
                settings: Some(audio_settings()),
                enabled: Some(true),
            })
            .await
            .map_err(e("audio source"))?;
    }
    // Only Lumen's own sink should reach the stream in this collection.
    if let Ok(specials) = client.inputs().specials().await {
        for name in [
            specials.desktop1,
            specials.desktop2,
            specials.mic1,
            specials.mic2,
        ]
        .into_iter()
        .flatten()
        {
            let _ = client.inputs().set_muted(name.as_str().into(), true).await;
        }
    }
    let how = match composition {
        Composition::Browser => "browser source",
        Composition::Native => "native OBS sources driven live by Lumen",
    };
    Ok((
        format!("{COLLECTION} ⁜ {SCENE} ready (1080×1920, 30 fps, {how})"),
        driver,
    ))
}

// ---------------------------------------------------------------------------------------
// Controller: a background thread with its own runtime; UI talks to it over channels.
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Refresh,
    LaunchObs,
    EnableWebsocket,
    Setup { overlay: String, socket: PathBuf },
    StartStream,
    StopStream,
    StartRecord,
    StopRecord,
    Monitor(bool),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub obs_running: bool,
    pub websocket_enabled: bool,
    pub connected: bool,
    pub collection_ready: bool,
    pub streaming: bool,
    pub stream_seconds: u64,
    pub congestion: f32,
    pub skipped_frames: u32,
    pub total_frames: u32,
    pub recording: bool,
    pub sink_ready: bool,
    pub monitoring: bool,
    /// Native composition is being driven live.
    pub driving: bool,
    /// Last action result or error, user-facing Turkish text.
    pub message: String,
}

pub struct Controller {
    actions: mpsc::Sender<Action>,
    statuses: mpsc::Receiver<Status>,
}

impl Controller {
    pub fn spawn() -> Self {
        let (actions, action_rx) = mpsc::channel::<Action>();
        let (status_tx, statuses) = mpsc::channel::<Status>();
        std::thread::Builder::new()
            .name("lumen-obs".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(run(action_rx, status_tx));
            })
            .expect("spawn OBS controller thread");
        Self { actions, statuses }
    }

    pub fn send(&self, action: Action) {
        let _ = self.actions.send(action);
    }

    /// Latest status, if any arrived since the last call.
    pub fn latest(&self) -> Option<Status> {
        self.statuses.try_iter().last()
    }
}

struct Session {
    client: Option<Client>,
    driver: Option<native::Driver>,
    socket: Option<PathBuf>,
}

async fn stage(socket: &std::path::Path) -> Option<lumen_protocol::StageState> {
    let socket = socket.to_owned();
    tokio::task::spawn_blocking(move || {
        lumen_protocol::request(
            &socket,
            lumen_protocol::Command::Status,
            Duration::from_millis(400),
        )
        .ok()
        .map(|r| r.stage)
    })
    .await
    .ok()
    .flatten()
}

async fn run(actions: mpsc::Receiver<Action>, statuses: mpsc::Sender<Status>) {
    let mut session = Session {
        client: None,
        driver: None,
        socket: None,
    };
    let mut message = String::new();
    let mut tick = 0u32;
    loop {
        for action in actions.try_iter().collect::<Vec<_>>() {
            if let Some(text) = perform(action, &mut session).await {
                message = text;
            }
        }
        // Drive the native composition from live stage state.
        if let (Some(driver), Some(client), Some(socket)) = (
            session.driver.as_mut(),
            session.client.as_ref(),
            session.socket.as_ref(),
        ) && let Some(state) = stage(socket).await
            && let Err(e) = driver.apply(client, &state).await
        {
            message = format!("Scene update failed: {e}");
            session.driver = None;
            session.client = None;
        }
        if tick.is_multiple_of(6) {
            let status = snapshot(&mut session, &message).await;
            if statuses.send(status).is_err() {
                break;
            }
        }
        tick = tick.wrapping_add(1);
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

async fn ensure_client(client: &mut Option<Client>) -> Result<&Client, String> {
    if client.is_none() {
        let config = read_websocket_config().ok_or("OBS WebSocket settings not found")?;
        if !config.enabled {
            return Err("OBS WebSocket is off".into());
        }
        *client = Some(connect(&config).await?);
    }
    Ok(client.as_ref().expect("connected above"))
}

async fn perform(action: Action, session: &mut Session) -> Option<String> {
    let client = &mut session.client;
    let result: Result<String, String> = match action {
        Action::Refresh => return None,
        Action::LaunchObs => launch_obs().map(|_| "Starting OBS…".into()),
        Action::EnableWebsocket => {
            enable_websocket().map(|_| "OBS WebSocket turned on; you can start OBS.".into())
        }
        Action::Monitor(on) => set_monitoring(on).map(|_| {
            if on {
                "Broadcast audio in your headphones"
            } else {
                "Monitoring off"
            }
            .into()
        }),
        Action::Setup { overlay, socket } => match ensure_sink() {
            Err(e) => Err(format!("Could not create the audio line: {e}")),
            Ok(()) => {
                let avatars = stage(&socket).await.map(|s| s.avatars).unwrap_or_default();
                match ensure_client(client).await {
                    Ok(c) => match setup(c, &overlay, &avatars).await {
                        Ok((text, driver)) => {
                            session.driver = driver;
                            session.socket = Some(socket);
                            Ok(text)
                        }
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                }
            }
        },
        Action::StartStream | Action::StopStream | Action::StartRecord | Action::StopRecord => {
            match ensure_client(client).await {
                Err(e) => Err(e),
                Ok(c) => match action {
                    Action::StartStream => {
                        c.streaming().start().await.map(|_| "Stream started".into())
                    }
                    Action::StopStream => {
                        c.streaming().stop().await.map(|_| "Stream stopped".into())
                    }
                    Action::StartRecord => c
                        .recording()
                        .start()
                        .await
                        .map(|_| "Recording started".into()),
                    _ => c
                        .recording()
                        .stop()
                        .await
                        .map(|path| format!("Recording saved: {path}")),
                }
                .map_err(|e| format!("OBS: {e}")),
            }
        }
    };
    if result.is_err() {
        // Drop a possibly dead connection; the next action reconnects.
        session.client = None;
        session.driver = None;
    }
    Some(result.unwrap_or_else(|e| e))
}

async fn snapshot(session: &mut Session, message: &str) -> Status {
    let driving = session.driver.is_some();
    let client = &mut session.client;
    let config = read_websocket_config();
    let mut status = Status {
        obs_running: obs_running(),
        websocket_enabled: config.as_ref().is_some_and(|c| c.enabled),
        sink_ready: sink_ready(),
        monitoring: monitoring(),
        message: message.to_owned(),
        driving,
        ..Status::default()
    };
    if !status.obs_running || !status.websocket_enabled {
        *client = None;
        return status;
    }
    let Ok(c) = ensure_client(client).await else {
        *client = None;
        return status;
    };
    status.connected = true;
    status.collection_ready = c
        .scene_collections()
        .current()
        .await
        .is_ok_and(|current| current == COLLECTION);
    match c.streaming().status().await {
        Ok(s) => {
            status.streaming = s.active;
            status.stream_seconds = s.duration.whole_seconds().max(0) as u64;
            status.congestion = s.congestion;
            status.skipped_frames = s.skipped_frames;
            status.total_frames = s.total_frames;
        }
        Err(_) => {
            *client = None;
            status.connected = false;
            return status;
        }
    }
    status.recording = c.recording().status().await.is_ok_and(|r| r.active);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_config_parsing_keeps_password_private_and_defaults_sane() {
        let c = parse_websocket_config(
            r#"{"server_enabled":true,"server_port":4460,"auth_required":true,"server_password":"s3cret"}"#,
        )
        .unwrap();
        assert_eq!(
            (c.enabled, c.port, c.password.as_deref()),
            (true, 4460, Some("s3cret"))
        );
        let no_auth =
            parse_websocket_config(r#"{"auth_required":false,"server_password":"x"}"#).unwrap();
        assert_eq!(
            (no_auth.enabled, no_auth.port, no_auth.password),
            (false, 4455, None)
        );
        assert!(parse_websocket_config("not json").is_none());
        // Debug output is the only place the struct could leak; the UI never formats it.
    }

    #[test]
    fn overlay_source_is_full_canvas_browser_source() {
        let s = overlay_settings("http://127.0.0.1:47800/");
        assert_eq!(
            (s["width"].as_u64(), s["height"].as_u64()),
            (Some(1080), Some(1920))
        );
        assert_eq!(s["url"], "http://127.0.0.1:47800/");
        assert_eq!(audio_settings()["device_id"], SINK);
    }
}
