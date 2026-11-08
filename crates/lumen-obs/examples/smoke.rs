//! End-to-end check against a running OBS: connect, set up the Lumen scene, verify it, and save
//! a screenshot of the program scene. Run against an isolated OBS profile (XDG_CONFIG_HOME).
//!
//!   cargo run -p lumen-obs --example smoke -- OUT.png [OVERLAY_URL]
use base64::Engine as _;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: smoke OUT.png [OVERLAY_URL]")?;
    let url = args.next().unwrap_or_else(|| lumen_obs::overlay_url(47800));
    let config = lumen_obs::read_websocket_config().ok_or("no obs-websocket config")?;
    let client = lumen_obs::connect(&config).await?;
    println!(
        "connected: obs-websocket {:?}",
        client
            .general()
            .version()
            .await
            .map_err(|e| e.to_string())?
            .obs_web_socket_version
    );
    let avatars = ["ogretmen".to_owned(), "fesli".to_owned()];
    let (text, _) = lumen_obs::setup(&client, &url, &avatars).await?;
    println!("{text}");
    // Idempotent: a second run must not duplicate anything.
    let (_, mut driver) = lumen_obs::setup(&client, &url, &avatars).await?;
    if let (Some(driver), Some(socket)) = (driver.as_mut(), std::env::var_os("LUMEN_SOCKET")) {
        // Drive the composition from the running Lumen runtime for a few seconds.
        for _ in 0..60 {
            let state = lumen_protocol::request(
                std::path::Path::new(&socket),
                lumen_protocol::Command::Status,
                std::time::Duration::from_secs(1),
            )
            .map_err(|e| e.to_string())?
            .stage;
            driver.apply(&client, &state).await?;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        println!("drove live state");
    }
    let collections = client
        .scene_collections()
        .list()
        .await
        .map_err(|e| e.to_string())?;
    let video = client
        .config()
        .video_settings()
        .await
        .map_err(|e| e.to_string())?;
    let inputs = client
        .inputs()
        .list(None)
        .await
        .map_err(|e| e.to_string())?;
    let scene = client
        .scenes()
        .current_program_scene()
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "collection: {} (all: {:?})",
        collections.current, collections.collections
    );
    println!(
        "canvas: {}x{} output {}x{} @ {}/{}",
        video.base_width,
        video.base_height,
        video.output_width,
        video.output_height,
        video.fps_numerator,
        video.fps_denominator
    );
    println!("program scene: {}", scene.id.name);
    for input in &inputs {
        println!("input: {} [{}]", input.id.name, input.kind);
    }
    assert_eq!(collections.current, lumen_obs::COLLECTION);
    assert_eq!((video.base_width, video.base_height), (1080, 1920));
    assert_eq!(
        inputs
            .iter()
            .filter(|i| i.id.name == lumen_obs::AUDIO_INPUT)
            .count(),
        1,
        "exactly one Lumen audio input"
    );
    assert!(
        inputs
            .iter()
            .filter(|i| i.id.name.starts_with("Lumen ⁜ "))
            .count()
            > 20
            || inputs.iter().any(|i| i.id.name == lumen_obs::OVERLAY_INPUT),
        "frame composition present"
    );
    // Let the browser source load and render a few frames.
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    let image = client
        .sources()
        .take_screenshot(obws::requests::sources::TakeScreenshot {
            source: lumen_obs::SCENE.into(),
            format: "png",
            width: Some(540),
            height: Some(960),
            compression_quality: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    let data = image.split_once(',').map_or(image.as_str(), |(_, d)| d);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())?;
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    println!("screenshot: {out}");
    Ok(())
}
