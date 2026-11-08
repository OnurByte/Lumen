//! Live probe: `cargo run -p lumen-platform-kwai --example probe -- QUERY...` lists users that are LIVE.
#[tokio::main]
async fn main() {
    let api = lumen_platform_kwai::api::KwaiApi::new(lumen_platform_kwai::api::DEFAULT_API_HOST, "ANDROID_0123456789abcdef", None).unwrap();
    for q in std::env::args().skip(1) {
        match api.search_user(&q).await {
            Ok(json) => {
                for u in json["users"].as_array().into_iter().flatten() {
                    if u["is_living"].as_bool() == Some(true) || !u["liveInfo"].is_null() {
                        println!("LIVE {q}: kwai_id={} name={} liveInfo={}", u["kwai_id"], u["user_name"], !u["liveInfo"].is_null());
                    }
                }
            }
            Err(e) => println!("{q}: {e}"),
        }
    }
}
