//! Private local operator endpoint, independent from model and speech work.
use crate::show::Show;
use lumen_protocol::{
    Command, MAX_FRAME_BYTES, PROTOCOL_VERSION, Response, read_frame, write_response,
};
use lumen_storage::Journal;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Notify, Semaphore},
};
#[cfg(windows)]
use tokio::net::{TcpListener as Listener, TcpStream as Stream};
#[cfg(unix)]
use tokio::net::{UnixListener as Listener, UnixStream as Stream};
use tokio_util::sync::CancellationToken;

pub struct Hub {
    pub status: Mutex<Response>,
    pub stage: Arc<StdMutex<lumen_protocol::StageState>>,
    pub audience: StdMutex<lumen_audience::AudienceQueue>,
    pub active: StdMutex<Option<CancellationToken>>,
    pub changed: Notify,
    pub shutdown: CancellationToken,
    pub clock: Instant,
    pub session: String,
    attached: bool,
    pub available: AtomicBool,
    /// Operator asked to move to the next topic; consumed by the show loop.
    pub skip_topic: AtomicBool,
    /// Show-clock millisecond when the current topic rotates (0 = no timer).
    pub topic_ends_ms: std::sync::atomic::AtomicU64,
    /// New question list from the operator, applied by the show loop.
    pub pending_topics: StdMutex<Option<lumen_protocol::TopicList>>,
    /// Show-clock millisecond the current session started (0 = not started).
    pub session_started_ms: std::sync::atomic::AtomicU64,
}
impl Hub {
    pub fn new(attached: bool) -> Self {
        Self {
            status: Mutex::new(Response {
                version: PROTOCOL_VERSION,
                paused: true,
                muted: false,
                stopped: false,
                error: None,
                stage: lumen_protocol::StageState::default(),
            }),
            stage: Arc::new(StdMutex::new(lumen_protocol::StageState::default())),
            audience: StdMutex::new(lumen_audience::AudienceQueue::default()),
            active: StdMutex::new(None),
            changed: Notify::new(),
            shutdown: CancellationToken::new(),
            clock: Instant::now(),
            session: format!(
                "show-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            attached,
            available: AtomicBool::new(false),
            skip_topic: AtomicBool::new(false),
            topic_ends_ms: std::sync::atomic::AtomicU64::new(0),
            pending_topics: StdMutex::new(None),
            session_started_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }
    pub fn cancel_turn(&self) {
        if let Ok(active) = self.active.lock()
            && let Some(cancel) = active.as_ref()
        {
            cancel.cancel();
        }
        self.changed.notify_one();
    }
    pub async fn fail(&self, error: String) {
        let mut status = self.status.lock().await;
        status.paused = true;
        status.error = Some(error);
        self.cancel_turn();
    }
}
struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn serve(socket: &Path, database: &Path, show: Option<Show>) -> Result<(), String> {
    let (listener, token) = bind(socket).await?;
    let _guard = SocketGuard(socket.to_owned());
    let journal = Arc::new(Journal::open(database).await.map_err(|e| e.to_string())?);
    let hub = Arc::new(Hub::new(show.is_some()));
    let show = show
        .map(|show| crate::show::spawn(show, database.to_owned(), hub.clone()))
        .transpose()?;
    tokio::spawn(crate::overlay::serve(hub.clone(), crate::overlay::port()));
    let slots = Arc::new(Semaphore::new(16));
    println!(
        "control endpoint ready: {} (show configured: {})",
        socket.display(),
        hub.attached
    );
    loop {
        let (stream, _) = tokio::select! {
            result = listener.accept() => result.map_err(|e| e.to_string())?,
            result = tokio::signal::ctrl_c() => { result.map_err(|e| e.to_string())?; break; }
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let hub = hub.clone();
        let journal = journal.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ =
                tokio::time::timeout(Duration::from_secs(2), handle(stream, hub, journal, token)).await;
        });
    }
    hub.shutdown.cancel();
    hub.cancel_turn();
    if let Some(show) = show {
        tokio::task::spawn_blocking(move || show.join())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|_| "show thread failed")?;
    }
    Ok(())
}

#[cfg(unix)]
async fn bind(socket: &Path) -> Result<(Listener, String), String> {
    use std::os::unix::fs::PermissionsExt;
    let parent = socket
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("socket requires a private parent directory")?;
    let metadata = std::fs::metadata(parent).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err("socket parent must be a private directory (mode 0700)".into());
    }
    let listener = Listener::bind(socket).map_err(|e| e.to_string())?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok((listener, String::new()))
}

/// Loopback plus a random token in a file only this user can read (see `lumen_protocol`).
#[cfg(windows)]
async fn bind(socket: &Path) -> Result<(Listener, String), String> {
    let listener = Listener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    std::fs::write(socket, format!("{port} {token}")).map_err(|e| e.to_string())?;
    Ok((listener, token))
}

async fn handle(mut stream: Stream, hub: Arc<Hub>, journal: Arc<Journal>, token: String) -> io::Result<()> {
    if !token.is_empty() {
        let mut sent = vec![0; token.len()];
        stream.read_exact(&mut sent).await?;
        if sent != token.as_bytes() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "bad token"));
        }
    }
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&header);
    frame.resize(length + 4, 0);
    stream.read_exact(&mut frame[4..]).await?;
    let command = read_frame(&mut io::Cursor::new(frame))?.command;
    if let Command::Audience(input) = command {
        let now = hub.clock.elapsed().as_millis() as u64;
        let result = hub
            .audience
            .lock()
            .map_err(|_| io::Error::other("audience unavailable"))?
            .push(
                lumen_audience::Comment {
                    id: input.id,
                    user: input.user,
                    text: input.text,
                    received_ms: now,
                },
                now,
            );
        let mut response = hub.status.lock().await.clone();
        response.error = result
            .err()
            .map(|error| format!("audience rejected: {error:?}"));
        return respond(&mut stream, &hub, response).await;
    }

    {
        let mut state = hub.status.lock().await;
        if command != Command::Status {
            state.error = None;
        }
        match command {
            Command::Status | Command::Audience(_) => {}
            Command::SetTopics(ref list) if hub.attached && list.valid() => {
                if let Ok(mut pending) = hub.pending_topics.lock() {
                    *pending = Some(list.clone());
                }
                hub.changed.notify_one();
            }
            Command::SetTopics(_) => {
                state.error = Some("question list rejected: 1-64 questions, 1-180 minutes".into())
            }
            Command::Pause => state.paused = true,
            Command::Resume if hub.attached && !hub.available.load(Ordering::Acquire) => {
                state.error = Some(
                    "show backend not ready; check setup and restart after initialization failure"
                        .into(),
                )
            }
            Command::Resume if !state.stopped => state.paused = false,
            Command::MuteAll => {
                state.muted = !state.muted;
                if state.muted {
                    state.paused = true;
                }
            }
            Command::EmergencyStop => {
                state.stopped = true;
                state.paused = true;
                state.muted = true;
            }
            Command::SkipTurn if hub.attached => {}
            Command::SkipTopic if hub.attached => hub.skip_topic.store(true, Ordering::Release),
            Command::SkipTurn | Command::SkipTopic | Command::StopStream => {
                state.error = Some("requested show/OBS action is not available".into())
            }
            Command::Resume => {
                state.error = Some("emergency stop is latched; restart the runtime".into())
            }
        }
    }
    if command != Command::Status {
        let accepted = hub.status.lock().await.error.is_none();
        if accepted {
            hub.cancel_turn();
        }
        if journal
            .append(
                &hub.session,
                hub.clock.elapsed().as_millis() as u64,
                "control",
                &format!(
                    "{command:?}: {}",
                    if accepted { "accepted" } else { "rejected" }
                ),
            )
            .await
            .is_err()
        {
            hub.fail("journal write failed; runtime paused".into())
                .await;
        }
    }
    let response = hub.status.lock().await.clone();
    respond(&mut stream, &hub, response).await
}

async fn respond(stream: &mut Stream, hub: &Hub, mut response: Response) -> io::Result<()> {
    response.stage = hub
        .stage
        .lock()
        .map_err(|_| io::Error::other("stage unavailable"))?
        .clone();
    let ends = hub.topic_ends_ms.load(Ordering::Acquire);
    let now = hub.clock.elapsed().as_millis() as u64;
    let started = hub.session_started_ms.load(Ordering::Acquire);
    response.stage.session_elapsed_s = if started == 0 {
        0
    } else {
        (now.saturating_sub(started) / 1000).min(u64::from(u32::MAX)) as u32
    };
    response.stage.topic_remaining_s = if ends == 0 {
        0
    } else {
        (ends.saturating_sub(now) / 1000).min(u64::from(u32::MAX)) as u32
    };
    let mut bytes = Vec::new();
    write_response(&mut bytes, &response)?;
    stream.write_all(&bytes).await
}
