//! Full Kwai Studio provisioning flow against a local fake Studio (request shapes from studio.kwai.com's JS).
use lumen_live::{AccountId, Credentials, LiveTarget, Platform, PlatformAdapter};
use lumen_platform_kwai::KwaiAdapter;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn fake_studio(log: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 { return; }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    let Some(head_end) = text.find("\r\n\r\n") else { continue };
                    let len = text[..head_end].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if buf.len() < head_end + 4 + len { continue; }
                    let head = &text[..head_end];
                    let body = String::from_utf8_lossy(&buf[head_end + 4..head_end + 4 + len]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap().to_owned();
                    let authed = head.lines().any(|l| l.eq_ignore_ascii_case("cookie: kpn=KWAI; userId=1; session=SECRET"));
                    log.lock().unwrap().push(format!("{path} | {}", if body.contains("name=\"streamType\"") { "multipart" } else { body.as_str() }));
                    let reply = if !authed {
                        r#"{"result":109,"error_msg":"login"}"#.to_string()
                    } else if path.contains("checkPermission") {
                        r#"{"result":1,"hasPermission":true,"userInfo":{"ksId":777,"kpn":4,"country":"BR"}}"#.into()
                    } else if path.contains("livingRoom") {
                        assert!(path.contains("authorId=777"));
                        r#"{"result":1,"curRoomVo":{}}"#.into()
                    } else if path.contains("startLive") || path.contains("stopLive") {
                        r#"{"result":1}"#.into()
                    } else if path.contains("transcodeResource") {
                        r#"{"result":1,"transcodeResources":[{"key":"SG","name":"Singapore"}]}"#.into()
                    } else if path.contains("createRoom") {
                        assert!(body.contains("name=\"pushNodes\"\r\n\r\nSG") && body.contains("name=\"streamType\"\r\n\r\n2") && body.contains("name=\"kpn\"\r\n\r\n4") && body.contains("name=\"cover\""));
                        r#"{"result":1,"createRoomResultVo":{"liveRoomId":"R42","autoStartTime":0}}"#.into()
                    } else if path.contains("cloudLive/room") {
                        r#"{"result":1,"data":{"liveRoomId":"R42","roomStatus":0,"pushUrl":["rtmp://push.kwai.test/live/KEY-42"]}}"#.into()
                    } else {
                        r#"{"result":500}"#.into()
                    };
                    let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
                    let _ = sock.write_all(resp.as_bytes()).await;
                    return;
                }
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn provisions_starts_and_stops_a_studio_room() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let host = fake_studio(log.clone()).await;
    let cover = std::env::temp_dir().join(format!("kwai-cover-{}.png", std::process::id()));
    std::fs::write(&cover, b"\x89PNG fake").unwrap();
    let mut target = LiveTarget { account_id: AccountId("a".into()), platform: Platform::KwaiGlobal, handle: "h".into(), settings: Default::default() };
    target.settings.insert("studio_host".into(), host);
    target.settings.insert("studio_cover".into(), cover.display().to_string());
    let creds = Credentials::new().with("studio_cookie", "kpn=KWAI; userId=1; session=SECRET");

    let report = KwaiAdapter.check_broadcast(&target, &creds).await.unwrap();
    assert!(report.contains("kpn 4") && report.contains("new room"), "{report}");
    log.lock().unwrap().clear();

    let ep = KwaiAdapter.provision_broadcast(&target, &creds).await.unwrap();
    assert_eq!((ep.ingest_url.as_str(), ep.stream_key.expose(), ep.room_id.as_deref()), ("rtmp://push.kwai.test/live", "KEY-42", Some("R42")));
    KwaiAdapter.start_broadcast(&target, &creds, &ep).await.unwrap();
    KwaiAdapter.stop_broadcast(&target, &creds, &ep).await.unwrap();
    let calls = log.lock().unwrap().clone();
    let paths: Vec<_> = calls.iter().map(|c| c.split('?').next().unwrap().rsplit('/').next().unwrap().split(' ').next().unwrap().to_owned()).collect();
    assert_eq!(paths, ["checkPermission", "livingRoom", "transcodeResource", "createRoom", "room", "room", "checkPermission", "startLive", "stopLive"]);
    assert!(calls[7].contains(r#""liveRoomId":"R42""#) && calls[7].contains(r#""noStreamCheckTime":2"#) && calls[7].contains(r#""kpn":4"#));

    // Pasting the whole header line works too.
    let prefixed = Credentials::new().with("studio_cookie", "Cookie: kpn=KWAI; userId=1; session=SECRET");
    KwaiAdapter.check_broadcast(&target, &prefixed).await.unwrap();

    let expired = Credentials::new().with("studio_cookie", "session=OLD");
    assert!(matches!(KwaiAdapter.provision_broadcast(&target, &expired).await, Err(lumen_live::AdapterError::Auth(_))));
    std::fs::remove_file(cover).ok();
}
