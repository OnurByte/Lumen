use lumen_accounts::{AccountCenter, MasterKey, NewAccount, StoreError};
use lumen_live::{AccountId, Platform};

fn new(platform: Platform, label: &str, handle: &str, group: Option<&str>) -> NewAccount {
    NewAccount { platform, label: label.into(), handle: handle.into(), group: group.map(Into::into) }
}

#[tokio::test]
async fn unlimited_accounts_isolated_credentials_and_broadcast_gate() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("accounts.sqlite");
    let key_path = dir.path().join("master.key");
    let center = AccountCenter::open(&db, MasterKey::load_or_create(&key_path).unwrap()).await.unwrap();

    let mut ids = Vec::new();
    for i in 0..30 {
        let platform = Platform::ALL[i % 3];
        let a = center.add(new(platform, &format!("acct-{i}"), &format!("user_{i}"), Some(if i % 2 == 0 { "main" } else { "backup" }))).await.unwrap();
        ids.push(a.id);
    }
    assert_eq!(center.list().await.unwrap().len(), 30);
    assert_eq!(center.list_group("main").await.unwrap().len(), 15);
    assert!(matches!(center.add(new(Platform::TikTok, "acct-0", "x", None)).await, Err(StoreError::Invalid(_))));
    assert!(center.add(new(Platform::TikTok, "bad", "no spaces", None)).await.is_err());

    center.set_credential(&ids[0], "cookie", "sessionid=AAA-SECRET").await.unwrap();
    center.set_credential(&ids[1], "cookie", "sessionid=BBB-SECRET").await.unwrap();
    assert_eq!(center.credentials(&ids[0]).await.unwrap().get("cookie").unwrap().expose(), "sessionid=AAA-SECRET");
    assert_eq!(center.credentials(&ids[1]).await.unwrap().get("cookie").unwrap().expose(), "sessionid=BBB-SECRET");
    assert!(center.credentials(&ids[2]).await.unwrap().is_empty());
    assert_eq!(center.get(&ids[0]).await.unwrap().credential_keys, vec!["cookie".to_string()]);

    // Secrets never appear in plaintext in the database files.
    drop(center);
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!bytes.windows(10).any(|w| w == b"AAA-SECRET"), "plaintext secret on disk");
    }
    let center = AccountCenter::open(&db, MasterKey::load_or_create(&key_path).unwrap()).await.unwrap();
    assert_eq!(center.credentials(&ids[0]).await.unwrap().get("cookie").unwrap().expose(), "sessionid=AAA-SECRET");

    // A different master key cannot decrypt.
    let other_db = dir.path().join("copy.sqlite");
    drop(center);
    std::fs::copy(&db, &other_db).unwrap();
    let wrong = AccountCenter::open(&other_db, MasterKey::from_bytes([1; 32])).await.unwrap();
    assert!(wrong.credentials(&ids[0]).await.is_err());
    let center = AccountCenter::open(&db, MasterKey::load_or_create(&key_path).unwrap()).await.unwrap();

    // Broadcast requires explicit authorization.
    assert!(center.set_broadcast_enabled(&ids[3], true).await.is_err());
    center.set_broadcast(&ids[3], Some("rtmps://ingest.example/live"), Some(3000), None).await.unwrap();
    assert!(center.set_broadcast(&ids[3], Some("http://x"), None, None).await.is_err());
    center.set_broadcast_authorized(&ids[3], true).await.unwrap();
    center.set_broadcast_enabled(&ids[3], true).await.unwrap();
    let b = center.get(&ids[3]).await.unwrap().broadcast;
    assert!(b.authorized && b.enabled && b.video_bitrate_kbps == 3000);
    center.set_broadcast_authorized(&ids[3], false).await.unwrap();
    assert!(!center.get(&ids[3]).await.unwrap().broadcast.enabled, "revoking authorization disables broadcast");

    // Switching, scenes and removal.
    center.set_active(&ids[5]).await.unwrap();
    assert_eq!(center.active().await.unwrap().unwrap().id, ids[5]);
    let scene = center.upsert_scene("main-show", &[ids[0].clone(), ids[1].clone(), ids[2].clone()]).await.unwrap();
    assert_eq!(scene.destinations.len(), 3);
    assert!(center.upsert_scene("bad", &[AccountId("missing".into())]).await.is_err());
    center.remove(&ids[5]).await.unwrap();
    center.remove(&ids[0]).await.unwrap();
    assert!(center.active().await.unwrap().is_none());
    assert_eq!(center.scene("main-show").await.unwrap().destinations.len(), 2);
    assert!(matches!(center.credentials(&ids[0]).await, Err(StoreError::NotFound(_))));
    assert_eq!(center.resolve("acct-7").await.unwrap().id, ids[7]);
    assert_eq!(center.resolve(&ids[8].as_str()[..8]).await.unwrap().id, ids[8]);
}
