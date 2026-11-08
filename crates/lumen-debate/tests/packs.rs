use lumen_debate::pack::ShowPack;
const EXAMPLE: &str = include_str!("../../../packs/personas/history-clash.toml");
#[test]
fn example_has_independent_personas_positions_models_and_voices() {
    let pack = ShowPack::parse(EXAMPLE).unwrap();
    assert_eq!(pack.name, "History Clash");
    assert_ne!(pack.agents[0].persona, pack.agents[1].persona);
    assert_ne!(pack.agents[0].position, pack.agents[1].position);
    assert_ne!(pack.agents[0].voice, pack.agents[1].voice);
    assert_eq!(pack.agents[0].max_tokens, 256);
}
#[test]
fn invalid_or_unknown_settings_rejected() {
    for invalid in [
        EXAMPLE.replace("temperature = 0.7", "temperature = nan"),
        EXAMPLE.replace("max_tokens = 256", "max_tokens = 0"),
        EXAMPLE.replace("name = \"Deniz\"", "name = \"Ada\""),
        EXAMPLE.replace("free_debate", "unknown"),
        format!("{EXAMPLE}\nsecret_key = 'oops'"),
        EXAMPLE.replace("http://127.0.0.1:11434", "http://example.com"),
    ] {
        assert!(ShowPack::parse(&invalid).is_err(), "{invalid}");
    }
    assert!(ShowPack::parse(&"x".repeat(65537)).is_err());
}
#[test]
fn topic_rotation_is_bounded_and_ordered() {
    let mut pack = ShowPack::parse(EXAMPLE).unwrap();
    pack.topics = vec![
        "İkinci konu".into(),
        pack.topic.clone(),
        "Üçüncü konu".into(),
    ];
    pack.topic_minutes = 10;
    let reparsed = ShowPack::parse(&toml::to_string(&pack).unwrap()).unwrap();
    let rotation = reparsed.rotation();
    assert_eq!(
        rotation.len(),
        3,
        "duplicates of the opening topic are dropped"
    );
    assert_eq!(rotation[0], pack.topic);
    assert_eq!(rotation[2], "Üçüncü konu");
    pack.topic_minutes = 0;
    assert!(pack.check().is_err());
    pack.topic_minutes = 10;
    pack.topics.push(" ".into());
    assert!(pack.check().is_err());
}
