use lumen_debate::pipeline::Segments;
#[test]
fn splits_streaming_turkish_without_breaking_abbreviations_or_numbers() {
    let mut segments = Segments::default();
    assert!(
        segments
            .push("Dr. Ayşe 15.10.2026 tarihinde geldi.")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        segments.push(" Peki neden? ").unwrap(),
        vec!["Dr. Ayşe 15.10.2026 tarihinde geldi.", "Peki neden?"]
    );
    assert!(segments.finish().unwrap().is_none());
}
#[test]
fn bounds_unpunctuated_output_and_rejects_control_characters() {
    let mut segments = Segments::default();
    assert!(segments.push("bad\u{1b}control").is_err());
    let mut segments = Segments::default();
    let output = segments.push(&"ğ".repeat(300)).unwrap();
    assert!(output.iter().all(|segment| segment.len() <= 512));
    assert!(segments.finish().unwrap().unwrap().len() <= 512);
}
