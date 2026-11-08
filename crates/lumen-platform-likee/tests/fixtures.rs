use lumen_live::{AdapterError, Credentials, LiveTarget, Platform, PlatformAdapter, AccountId};
use lumen_platform_likee::*;
use serde_json::Value;

fn load(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}

#[test]
fn parses_reference_shaped_fixtures() {
    let html = std::fs::read_to_string(format!("{}/tests/fixtures/profile.html", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let p = parse_profile_html(&html).unwrap();
    assert_eq!((p.user_id.as_str(), p.handle.as_str(), p.followers, p.bio.as_deref()), ("30007", "Likee_USA", Some(33493455), Some("bio {with} braces")));

    let v = parse_videos(&load("videos.json")).unwrap();
    assert_eq!(v[0].id, "7046501523492703838");
    assert_eq!((v[0].plays, v[0].shares), (58557, 723));
    assert_eq!(v[0].hashtags, vec!["Aurea"]);

    let c = parse_comments(&load("comments.json")).unwrap();
    assert_eq!((c[0].text.as_str(), c[0].likes, c[0].created_at), ("nice video", 14, Some(1615824458)));
    let h = parse_hashtags(&load("hashtags.json")).unwrap();
    assert_eq!((h[0].name.as_str(), h[0].videos), ("dance", 1200));
    assert!(data(&serde_json::json!({"code": 1, "msg": "bad"})).is_err());
}

#[tokio::test]
async fn live_requires_a_likee_share_link() {
    let a = LikeeAdapter;
    assert_eq!(a.capabilities().live_events, lumen_live::Support::Experimental);
    let mut t = LiveTarget { account_id: AccountId("a".into()), platform: Platform::LikeeGlobal, handle: "x".into(), settings: Default::default() };
    assert!(matches!(a.open_live(&t, &Credentials::new(), Default::default()).await, Err(AdapterError::NotFound(_))));
    t.settings.insert("share_url".into(), "https://evil.example/x".into());
    assert!(matches!(a.open_live(&t, &Credentials::new(), Default::default()).await, Err(AdapterError::NotFound(_))));
    assert!(matches!(a.provision_broadcast(&t, &Credentials::new()).await, Err(AdapterError::Unsupported(_))));
}
