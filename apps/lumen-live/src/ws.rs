//! Local WebSocket event distribution (one JSON `LiveEvent` per message) for overlays/tools.
use futures_util::SinkExt;
use lumen_live::hub::EventHub;
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub async fn serve(addr: &str, hub: EventHub, cancel: CancellationToken) -> Result<(), String> {
    let listener = TcpListener::bind(addr).await.map_err(|e| format!("bind {addr}: {e}"))?;
    if !listener.local_addr().map_err(|e| e.to_string())?.ip().is_loopback() {
        return Err("event WebSocket must bind a loopback address".into());
    }
    tokio::spawn(async move {
        loop {
            let (sock, _) = tokio::select! { _ = cancel.cancelled() => break, r = listener.accept() => match r { Ok(x) => x, Err(_) => continue } };
            let (mut rx, cancel) = (hub.subscribe(), cancel.clone());
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await else { return };
                loop {
                    let ev = tokio::select! { _ = cancel.cancelled() => break, ev = rx.recv() => ev };
                    match ev {
                        Ok(ev) => {
                            let Ok(json) = serde_json::to_string(&ev) else { continue };
                            if ws.send(Message::text(json)).await.is_err() { break; }
                        }
                        Err(RecvError::Lagged(_)) => continue, // slow client: skip ahead, never block the hub
                        Err(RecvError::Closed) => break,
                    }
                }
                let _ = ws.close(None).await;
            });
        }
    });
    Ok(())
}
