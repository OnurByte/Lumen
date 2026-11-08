//! Non-blocking control-socket client shared by Studio windows. Requests run on short-lived
//! threads with a bounded number in flight; results are drained on the UI thread.
use gpui::KeyDownEvent;
use lumen_protocol::{Command, Response};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

pub type Reply = (Command, Result<Response, String>);

pub struct Link {
    pub socket: PathBuf,
    incoming: mpsc::Receiver<Reply>,
    sender: mpsc::SyncSender<Reply>,
    pending: Arc<AtomicUsize>,
}

impl Link {
    pub fn new(socket: PathBuf) -> Self {
        let (sender, incoming) = mpsc::sync_channel(32);
        Self {
            socket,
            incoming,
            sender,
            pending: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::Relaxed) == 0
    }

    pub fn drain(&self) -> Vec<Reply> {
        self.incoming.try_iter().collect()
    }

    /// Operator controls may use 4 slots (emergency stop 5); audience input only 2, so chat
    /// traffic can never crowd out a control.
    pub fn send(&self, command: Command) {
        let limit = match command {
            Command::EmergencyStop => 5,
            Command::Audience(_) => 2,
            _ => 4,
        };
        let mut count = self.pending.load(Ordering::Relaxed);
        loop {
            if count >= limit {
                return;
            }
            match self.pending.compare_exchange_weak(
                count,
                count + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => count = current,
            }
        }
        let socket = self.socket.clone();
        let sender = self.sender.clone();
        let pending = self.pending.clone();
        std::thread::spawn(move || {
            let result =
                lumen_protocol::request(&socket, command.clone(), Duration::from_millis(1500))
                    .map_err(|e| e.to_string());
            let _ = sender.try_send((command, result));
            pending.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

/// Global operator shortcuts shared by every Studio window.
pub fn shortcut(event: &KeyDownEvent) -> Option<Command> {
    match event.keystroke.key.as_str() {
        "f2" => Some(Command::Pause),
        "f3" => Some(Command::Resume),
        "f4" => Some(Command::MuteAll),
        "f8" => Some(Command::SkipTurn),
        "f9" => Some(Command::SkipTopic),
        "f10" => Some(Command::StopStream),
        "escape" if event.keystroke.modifiers.control && event.keystroke.modifiers.shift => {
            Some(Command::EmergencyStop)
        }
        _ => None,
    }
}
