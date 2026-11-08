//! Lists the input kinds the running OBS offers (diagnostics for `lumen-obs`).
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let config = lumen_obs::read_websocket_config().ok_or("no obs-websocket config")?;
    let client = lumen_obs::connect(&config).await?;
    for kind in client
        .inputs()
        .list_kinds(false)
        .await
        .map_err(|e| e.to_string())?
    {
        println!("{kind}");
    }
    Ok(())
}
