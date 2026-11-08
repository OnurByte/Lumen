//! `lumen-live`: Account Center, concurrent LIVE connections and multi-destination broadcast.
mod ws;

use lumen_accounts::{Account, AccountCenter, NewAccount};
use lumen_broadcast::Destination;
use lumen_live::hub::{EventHub, HubConfig};
use lumen_live::moderation::{ModerationConfig, Moderator};
use lumen_live::supervisor::Backoff;
use lumen_live::{LiveManager, Platform, PlatformAdapter};
use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const USAGE: &str = "lumen-live — multi-platform, multi-account LIVE for Lumen

ACCOUNTS
  accounts add --platform tiktok|kwai|likee --label NAME --handle HANDLE [--group G]
  accounts list [--group G]          accounts show REF        accounts remove REF
  accounts rename REF NAME           accounts group REF [G]   accounts use REF
  accounts enable REF | disable REF  accounts live REF on|off
  accounts set REF KEY [VALUE]       (settings: watch, proxy, region, language, device_id, socket,
                                      share_url (Likee LIVE link), studio_cover, studio_caption (Kwai Studio))
  accounts secret REF NAME           (reads the value from stdin: cookie, stream_key, studio_cookie)
  accounts broadcast REF --ingest rtmps://… [--video-kbps N] [--audio-kbps N]
  accounts authorize-broadcast REF --confirm   |   accounts revoke-broadcast REF
  accounts broadcast-on REF | broadcast-off REF
SCENES
  scene set NAME REF...    scene list
RUN
  connect [--group G] [--account REF]... [--socket LUMEND_SOCKET] [--events-ws 127.0.0.1:PORT] [--status-secs N]
  broadcast --scene NAME [--listen 127.0.0.1:9100] [--ffmpeg PATH]
LOOKUPS
  profile PLATFORM HANDLE      capabilities
  likee search KEYWORD [COUNTRY] | user UID | videos CREATOR_ID | trending [COUNTRY] | hashtags [COUNTRY]
        | hashtag ID [COUNTRY] | comments VIDEO_ID

REF is an account label, id or unique id prefix. Data lives in $LUMEN_HOME (default ~/.local/share/lumen).";

struct Args(Vec<String>);
impl Args {
    fn flag(&mut self, name: &str) -> Option<String> {
        let i = self.0.iter().position(|a| a == name)?;
        self.0.remove(i);
        (i < self.0.len()).then(|| self.0.remove(i))
    }
    fn flags(&mut self, name: &str) -> Vec<String> {
        std::iter::from_fn(|| self.flag(name)).collect()
    }
    fn switch(&mut self, name: &str) -> bool {
        self.0.iter().position(|a| a == name).map(|i| self.0.remove(i)).is_some()
    }
    fn pos(&mut self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.remove(0))
    }
    fn need(&mut self, what: &str) -> Result<String, String> {
        self.pos().ok_or_else(|| format!("missing {what}\n\n{USAGE}"))
    }
}

async fn center() -> Result<AccountCenter, String> {
    lumen_accounts::open_default().await
}

fn adapter(p: Platform) -> Arc<dyn PlatformAdapter> {
    match p {
        Platform::TikTok => Arc::new(lumen_platform_tiktok::TikTokAdapter::default()),
        Platform::KwaiGlobal => Arc::new(lumen_platform_kwai::KwaiAdapter),
        Platform::LikeeGlobal => Arc::new(lumen_platform_likee::LikeeAdapter),
    }
}

fn print_account(a: &Account, active: bool) {
    let b = &a.broadcast;
    println!(
        "{} {:<20} {:<12} @{:<20} group={:<10} {} live={} broadcast={} secrets=[{}] id={}",
        if active { "*" } else { " " },
        a.label, a.platform.as_str(), a.handle, a.group.as_deref().unwrap_or("-"),
        if a.enabled { "enabled " } else { "disabled" }, if a.live_enabled { "on" } else { "off" },
        if !b.authorized { "unauthorized".to_string() } else if b.enabled { format!("on({})", b.ingest_url.as_deref().unwrap_or("no ingest")) } else { "authorized,off".into() },
        a.credential_keys.join(","), a.id.short()
    );
}

fn read_secret() -> Result<String, String> {
    let tty = std::io::stdin().is_terminal();
    if tty {
        eprint!("value (input hidden): ");
        let _ = std::process::Command::new("stty").arg("-echo").status();
    }
    let mut line = String::new();
    let r = std::io::stdin().lock().read_line(&mut line);
    if tty {
        let _ = std::process::Command::new("stty").arg("echo").status();
        eprintln!();
    }
    r.map_err(|e| e.to_string())?;
    let v = line.trim_end_matches(['\n', '\r']).to_owned();
    if v.is_empty() { Err("empty value".into()) } else { Ok(v) }
}

async fn accounts(mut a: Args) -> Result<(), String> {
    let c = center().await?;
    let e = |e: lumen_accounts::StoreError| e.to_string();
    match a.need("accounts subcommand")?.as_str() {
        "add" => {
            let platform: Platform = a.flag("--platform").ok_or("--platform required")?.parse()?;
            let acct = c.add(NewAccount { platform, label: a.flag("--label").ok_or("--label required")?, handle: a.flag("--handle").ok_or("--handle required")?, group: a.flag("--group") }).await.map_err(e)?;
            print_account(&acct, false);
        }
        "list" => {
            let active = c.active().await.map_err(e)?.map(|a| a.id);
            let list = match a.flag("--group") { Some(g) => c.list_group(&g).await, None => c.list().await }.map_err(e)?;
            for acct in &list {
                print_account(acct, Some(&acct.id) == active.as_ref());
            }
            println!("{} account(s); groups: {:?}", list.len(), c.groups().await.map_err(e)?);
        }
        "show" => {
            let acct = c.resolve(&a.need("REF")?).await.map_err(e)?;
            print_account(&acct, false);
            println!("settings: {:?}", acct.settings);
            for (name, s) in adapter(acct.platform).capabilities().rows() {
                println!("  {name:<15} {s:?}");
            }
            if let Some(m) = c.metrics(&acct.id).await.map_err(e)? {
                println!("last metrics: {m}");
            }
        }
        "remove" => {
            let acct = c.resolve(&a.need("REF")?).await.map_err(e)?;
            c.remove(&acct.id).await.map_err(e)?;
            println!("removed {} and its credentials", acct.label);
        }
        "rename" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.rename(&acct.id, &a.need("NAME")?).await.map_err(e)?; }
        "group" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_group(&acct.id, a.pos().as_deref()).await.map_err(e)?; }
        "use" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_active(&acct.id).await.map_err(e)?; println!("active account: {}", acct.label); }
        cmd @ ("enable" | "disable") => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_enabled(&acct.id, cmd == "enable").await.map_err(e)?; }
        "live" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_live_enabled(&acct.id, a.need("on|off")? == "on").await.map_err(e)?; }
        "set" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; let key = a.need("KEY")?; c.set_setting(&acct.id, &key, a.pos().as_deref()).await.map_err(e)?; }
        "secret" => {
            let acct = c.resolve(&a.need("REF")?).await.map_err(e)?;
            let name = a.need("NAME")?;
            c.set_credential(&acct.id, &name, &read_secret()?).await.map_err(e)?;
            println!("stored encrypted credential '{name}' for {}", acct.label);
        }
        "broadcast" => {
            let acct = c.resolve(&a.need("REF")?).await.map_err(e)?;
            let kbps = |v: Option<String>| v.map(|s| s.parse::<u32>().map_err(|_| "bitrate must be a number".to_string())).transpose();
            c.set_broadcast(&acct.id, a.flag("--ingest").as_deref(), kbps(a.flag("--video-kbps"))?, kbps(a.flag("--audio-kbps"))?).await.map_err(e)?;
        }
        "authorize-broadcast" => {
            let acct = c.resolve(&a.need("REF")?).await.map_err(e)?;
            if !a.switch("--confirm") {
                return Err(format!("broadcasting to '{}' requires your confirmation that you own or are authorized to stream to this account and that its terms allow it; re-run with --confirm", acct.label));
            }
            c.set_broadcast_authorized(&acct.id, true).await.map_err(e)?;
        }
        "revoke-broadcast" => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_broadcast_authorized(&acct.id, false).await.map_err(e)?; }
        cmd @ ("broadcast-on" | "broadcast-off") => { let acct = c.resolve(&a.need("REF")?).await.map_err(e)?; c.set_broadcast_enabled(&acct.id, cmd == "broadcast-on").await.map_err(e)?; }
        other => return Err(format!("unknown accounts subcommand '{other}'\n\n{USAGE}")),
    }
    Ok(())
}

async fn scene(mut a: Args) -> Result<(), String> {
    let c = center().await?;
    match a.need("scene subcommand")?.as_str() {
        "set" => {
            let name = a.need("NAME")?;
            let mut ids = Vec::new();
            while let Some(r) = a.pos() {
                ids.push(c.resolve(&r).await.map_err(|e| e.to_string())?.id);
            }
            let s = c.upsert_scene(&name, &ids).await.map_err(|e| e.to_string())?;
            println!("scene '{}' → {} destination(s)", s.name, s.destinations.len());
        }
        "list" => {
            for s in c.scenes().await.map_err(|e| e.to_string())? {
                let mut labels = Vec::new();
                for id in &s.destinations {
                    labels.push(c.get(id).await.map(|a| a.label).unwrap_or_else(|_| id.to_string()));
                }
                println!("{}: {}", s.name, labels.join(", "));
            }
        }
        other => return Err(format!("unknown scene subcommand '{other}'")),
    }
    Ok(())
}

async fn connect(mut a: Args) -> Result<(), String> {
    let c = center().await?;
    let picked = a.flags("--account");
    let group = a.flag("--group");
    let socket = a.flag("--socket").map(PathBuf::from);
    let ws_addr = a.flag("--events-ws");
    let status_secs: u64 = a.flag("--status-secs").and_then(|s| s.parse().ok()).unwrap_or(15);
    let mut accounts = Vec::new();
    if picked.is_empty() {
        for acct in c.list().await.map_err(|e| e.to_string())? {
            if acct.enabled && acct.live_enabled && group.as_ref().is_none_or(|g| acct.group.as_ref() == Some(g)) {
                accounts.push(acct);
            }
        }
    } else {
        for r in picked {
            accounts.push(c.resolve(&r).await.map_err(|e| e.to_string())?);
        }
    }
    if accounts.is_empty() {
        return Err("no enabled LIVE accounts selected".into());
    }
    let cancel = CancellationToken::new();
    let (hub, inbox) = EventHub::start(HubConfig::default(), cancel.clone());
    let manager = Arc::new(LiveManager::new(hub.clone(), Backoff::default(), cancel.clone()));
    for p in Platform::ALL {
        manager.register_adapter(adapter(p));
    }
    for acct in &accounts {
        let creds = c.credentials(&acct.id).await.map_err(|e| e.to_string())?;
        manager.start(acct.live_target(), creds, &acct.label)?;
        println!("started {} ({} @{})", acct.label, acct.platform, acct.handle);
    }
    if let Some(addr) = ws_addr {
        ws::serve(&addr, hub.clone(), cancel.clone()).await?;
        println!("event WebSocket on ws://{addr}");
    }
    let bridge = match socket {
        Some(path) => tokio::spawn(lumen_live::bridge::forward_to_lumend(inbox, path, Moderator::new(ModerationConfig::default()))),
        None => tokio::spawn(async move {
            // No show attached: print moderated chat so the operator can watch the combined inbox.
            let mut inbox = inbox;
            let mut m = Moderator::new(ModerationConfig::default());
            while let Some(ev) = inbox.recv().await {
                if let (lumen_live::moderation::Verdict::Allow, lumen_live::EventKind::Chat { text }) = (m.review(&ev, std::time::Instant::now()), &ev.kind) {
                    println!("[{} {}] {}: {}", ev.platform.short(), ev.account_id.short(), ev.user.as_ref().map_or("?", |u| u.display_name.as_str()), text);
                }
            }
            Default::default()
        }),
    };
    let mut tick = tokio::time::interval(Duration::from_secs(status_secs.max(1)));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => {
                for h in manager.health() {
                    let _ = c.record_metrics(&h).await;
                    println!("status {:<20} {:<12} {:<40} recv={} fwd={} drop={} reconnects={}", h.label, h.platform.as_str(), serde_json::to_string(&h.state).unwrap_or_default(), h.events_received, h.events_forwarded, h.dropped_backpressure + h.dropped_rate_limit, h.reconnects);
                }
            }
        }
    }
    manager.shutdown(Duration::from_secs(3)).await;
    for h in manager.health() {
        let _ = c.record_metrics(&h).await;
    }
    cancel.cancel();
    if let Ok(Ok(stats)) = tokio::time::timeout(Duration::from_secs(3), bridge).await {
        println!("inbox: {}", serde_json::to_string(&stats).unwrap_or_default());
    }
    Ok(())
}

async fn broadcast(mut a: Args) -> Result<(), String> {
    let c = center().await?;
    let scene = c.scene(&a.flag("--scene").ok_or("--scene required")?).await.map_err(|e| e.to_string())?;
    let listen = a.flag("--listen").unwrap_or_else(|| "127.0.0.1:9100".into());
    let ffmpeg = a.flag("--ffmpeg").unwrap_or_else(|| "ffmpeg".into());
    let mut destinations = Vec::new();
    let mut provisioned = Vec::new();
    for id in &scene.destinations {
        let acct = c.get(id).await.map_err(|e| e.to_string())?;
        let b = &acct.broadcast;
        // Reading LIVE events never implies permission to publish: every gate is checked here.
        let skip = |why: &str| println!("skip {}: {why}", acct.label);
        if !acct.enabled { skip("account disabled"); continue; }
        if !b.authorized { skip("not authorized for broadcasting (accounts authorize-broadcast)"); continue; }
        if !b.enabled { skip("broadcast is off (accounts broadcast-on)"); continue; }
        let creds = c.credentials(id).await.map_err(|e| e.to_string())?;
        // A key the operator entered wins; otherwise ask the platform for one (Kwai Studio).
        let (ingest, key) = match (b.ingest_url.clone(), creds.get("stream_key").cloned()) {
            (Some(ingest), Some(key)) => (ingest, key),
            _ => {
                let target = acct.live_target();
                match adapter(acct.platform).provision_broadcast(&target, &creds).await {
                    Ok(ep) => {
                        println!("{}: {} issued an ingest endpoint ({})", acct.label, acct.platform, ep.ingest_url);
                        let out = (ep.ingest_url.clone(), ep.stream_key.clone());
                        provisioned.push((acct.label.clone(), target, creds.clone(), ep));
                        out
                    }
                    Err(e) => { skip(&format!("no ingest URL + stream_key, and the platform could not issue one: {e}")); continue; }
                }
            }
        };
        destinations.push(Destination { account_id: acct.id.clone(), label: acct.label.clone(), platform: acct.platform, ingest_url: ingest, stream_key: key, video_kbps: b.video_bitrate_kbps, audio_kbps: b.audio_bitrate_kbps });
    }
    if destinations.is_empty() {
        return Err("no authorized, enabled destinations in this scene".into());
    }
    let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| format!("bind {listen}: {e}"))?;
    if !listener.local_addr().map_err(|e| e.to_string())?.ip().is_loopback() {
        return Err("ingest must listen on a loopback address".into());
    }
    println!("send MPEG-TS to tcp://{listen} (OBS: Settings → Output → Recording → Custom Output (FFmpeg), output to URL, container mpegts)");
    let cancel = CancellationToken::new();
    let relay = lumen_broadcast::start(listener, destinations, ffmpeg, cancel.clone());
    // Platform-side "go live" for provisioned rooms, once their video is actually flowing.
    let mut pending: Vec<_> = provisioned.iter().collect();
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => {
                let snapshot = relay.snapshot();
                for h in &snapshot {
                    println!("dest {:<20} {:<12} {:?} starts={} sent={}KiB err={}", h.label, h.platform.as_str(), h.state, h.starts, h.bytes_sent / 1024, h.last_error.as_deref().unwrap_or("-"));
                }
                let mut still = Vec::new();
                for p in pending {
                    let (label, target, creds, ep) = p;
                    let flowing = snapshot.iter().any(|h| &h.label == label && h.bytes_sent > 512 * 1024);
                    if !flowing { still.push(p); continue; }
                    match adapter(target.platform).start_broadcast(target, creds, ep).await {
                        Ok(()) => println!("{label}: LIVE started on {}", target.platform),
                        Err(e) if e.is_retryable() => { println!("{label}: not started yet ({e}); retrying"); still.push(p); }
                        Err(e) => println!("{label}: could not start LIVE: {e}"),
                    }
                }
                pending = still;
            }
        }
    }
    for (label, target, creds, ep) in &provisioned {
        match adapter(target.platform).stop_broadcast(target, creds, ep).await {
            Ok(()) => println!("{label}: LIVE stopped"),
            Err(e) => println!("{label}: stop failed, end it in the platform's own tools: {e}"),
        }
    }
    cancel.cancel();
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}

async fn likee(mut a: Args) -> Result<(), String> {
    let cl = lumen_platform_likee::LikeeClient::new(None).map_err(|e| e.to_string())?;
    let country = |a: &mut Args| a.pos().unwrap_or_else(|| "US".into());
    fn show<T: serde::Serialize>(r: Result<T, lumen_live::AdapterError>) -> Result<(), String> {
        println!("{}", serde_json::to_string_pretty(&r.map_err(|e| e.to_string())?).unwrap_or_default());
        Ok(())
    }
    match a.need("likee subcommand")?.as_str() {
        "videos" => show(cl.creator_videos(&a.need("CREATOR_ID")?, "", 30).await),
        "trending" => show(cl.trending_videos(&country(&mut a), "en", 0, 30).await),
        "hashtags" => show(cl.trending_hashtags(&country(&mut a), "en", 1, 50).await),
        "hashtag" => { let id = a.need("HASHTAG_ID")?; show(cl.hashtag_videos(&id, &country(&mut a), 1, 30).await) }
        "search" => show(cl.search_users(&a.need("KEYWORD")?, 20, &country(&mut a)).await),
        "user" => show(cl.user_info(&a.need("UID")?).await),
        "comments" => show(cl.video_comments(&a.need("VIDEO_ID")?, "en", "0", 49).await),
        other => Err(format!("unknown likee subcommand '{other}'")),
    }
}

async fn run() -> Result<(), String> {
    let mut a = Args(std::env::args().skip(1).collect());
    match a.pos().as_deref() {
        Some("accounts") => accounts(a).await,
        Some("scene") => scene(a).await,
        Some("connect") => connect(a).await,
        Some("broadcast") => broadcast(a).await,
        Some("likee") => likee(a).await,
        Some("profile") => {
            let p: Platform = a.need("PLATFORM")?.parse()?;
            let prof = adapter(p).profile(&a.need("HANDLE")?, &Default::default()).await.map_err(|e| e.to_string())?;
            println!("{}", serde_json::to_string_pretty(&prof).unwrap_or_default());
            Ok(())
        }
        Some("capabilities") => {
            for p in Platform::ALL {
                println!("{p}:");
                for (name, s) in adapter(p).capabilities().rows() {
                    println!("  {name:<15} {s:?}");
                }
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).with_writer(std::io::stderr).init();
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
