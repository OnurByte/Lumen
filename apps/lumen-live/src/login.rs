//! `accounts login`: the user logs in by hand in a throwaway Chromium-family window (so 2FA,
//! QR codes and captchas stay with them), and Lumen reads the resulting session over the
//! DevTools protocol. Lumen never sees the password; the window's profile is deleted afterwards.
use futures_util::{SinkExt, StreamExt};
use lumen_live::{LiveTarget, LoginSpec, PlatformAdapter, SecretString, SessionCapture};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

const BROWSERS: &[&str] = &["chromium", "google-chrome-stable", "google-chrome", "helium-browser", "brave", "brave-browser", "microsoft-edge-stable", "vivaldi-stable"];
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

fn find_browser() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    for app in ["Google Chrome", "Chromium", "Brave Browser", "Microsoft Edge"] {
        let bin = PathBuf::from(format!("/Applications/{app}.app/Contents/MacOS/{app}"));
        if bin.is_file() {
            return Ok(bin);
        }
    }
    #[cfg(windows)]
    for bin in [r"C:\Program Files\Google\Chrome\Application\chrome.exe", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"] {
        if Path::new(bin).is_file() {
            return Ok(bin.into());
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .flat_map(|dir| BROWSERS.iter().map(move |b| dir.join(b)))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no Chromium-family browser found (tried {}); pass --browser PATH", BROWSERS.join(", ")))
}

/// Deletes the login profile (cookies included) however the login ends.
struct ProfileDir(PathBuf);
impl Drop for ProfileDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Cdp {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(Message::text(msg.to_string())).await.map_err(|_| "browser window was closed".to_string())?;
        let wait = async {
            while let Some(frame) = self.ws.next().await {
                let Ok(Message::Text(text)) = frame else { continue };
                let reply: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                if reply.get("id").and_then(Value::as_u64) == Some(id) {
                    return match reply.get("error") {
                        Some(e) => Err(format!("{method}: {e}")),
                        None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
                    };
                }
            }
            Err("browser window was closed".to_string())
        };
        tokio::time::timeout(Duration::from_secs(15), wait).await.map_err(|_| format!("{method}: browser did not answer"))?
    }

    async fn attach_page(&mut self) -> Result<String, String> {
        let targets = self.call("Target.getTargets", json!({}), None).await?;
        let page = targets["targetInfos"].as_array().and_then(|l| l.iter().find(|t| t["type"] == "page")).ok_or("browser window was closed")?;
        let attached = self.call("Target.attachToTarget", json!({"targetId": page["targetId"], "flatten": true}), None).await?;
        attached["sessionId"].as_str().map(str::to_owned).ok_or_else(|| "no DevTools session".to_string())
    }

    async fn read_session(&mut self, capture: &SessionCapture, session: &str) -> Result<String, String> {
        match capture {
            SessionCapture::Cookies(url) => Ok(cookie_header(&self.call("Network.getCookies", json!({"urls": [url]}), Some(session)).await?)),
            SessionCapture::Script(expr) => {
                let out = self.call("Runtime.evaluate", json!({"expression": expr, "returnByValue": true}), Some(session)).await?;
                Ok(out["result"]["value"].as_str().unwrap_or("").to_owned())
            }
        }
    }
}

fn cookie_header(reply: &Value) -> String {
    reply["cookies"].as_array().map(|l| l.iter().filter_map(|c| Some(format!("{}={}", c["name"].as_str()?, c["value"].as_str()?))).collect::<Vec<_>>().join("; ")).unwrap_or_default()
}

/// `DevToolsActivePort` holds the port on line 1 and the browser endpoint path on line 2.
async fn devtools_url(profile: &Path) -> Result<String, String> {
    for _ in 0..200 {
        if let Ok(text) = tokio::fs::read_to_string(profile.join("DevToolsActivePort")).await
            && let Some((port, path)) = text.split_once('\n')
            && !path.trim().is_empty()
        {
            return Ok(format!("ws://127.0.0.1:{}{}", port.trim(), path.trim()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("the browser did not open a DevTools port; pass a Chromium-family browser with --browser".into())
}

/// Open the platform's login page and wait until the user has logged in.
pub async fn capture(adapter: &dyn PlatformAdapter, target: &LiveTarget, spec: &LoginSpec, browser: Option<String>, home: &Path) -> Result<SecretString, String> {
    let bin = browser.map_or_else(find_browser, |b| Ok(PathBuf::from(b)))?;
    let profile = ProfileDir(home.join(format!("login-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&profile.0);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&profile.0).map_err(|e| format!("cannot create login profile: {e}"))?;
    let mut child = tokio::process::Command::new(&bin)
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.0.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--new-window"])
        .arg(&spec.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", bin.display()))?;
    let mut browser_ws = None;
    let result = async {
        let (ws, _) = connect_async(devtools_url(&profile.0).await?).await.map_err(|e| format!("cannot reach the browser: {e}"))?;
        let cdp = browser_ws.insert(Cdp { ws, next_id: 0 });
        let mut session = cdp.attach_page().await?;
        let mut last = String::new();
        println!("log in in the browser window that just opened (waiting up to 10 minutes, Ctrl-C cancels)");
        let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => return Err("login cancelled".to_string()),
                _ = tokio::time::sleep_until(deadline) => return Err("timed out waiting for the login".to_string()),
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
            // The tab can be replaced while the login redirects; a dead browser fails the re-attach.
            let value = match cdp.read_session(&spec.capture, &session).await {
                Ok(v) => v,
                Err(_) => { session = cdp.attach_page().await?; continue; }
            };
            if !value.is_empty() && value != last {
                last.clone_from(&value);
                let secret = SecretString::new(value);
                if adapter.session_valid(target, &secret).await {
                    return Ok(secret);
                }
            }
        }
    }
    .await;
    // Killing only the main process leaves helpers writing into the profile, so it could not be deleted.
    if let Some(mut cdp) = browser_ws {
        let _ = tokio::time::timeout(Duration::from_secs(3), cdp.call("Browser.close", json!({}), None)).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    let _ = child.kill().await;
    for _ in 0..50 {
        if std::fs::remove_dir_all(&profile.0).is_ok() || !profile.0.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn joins_cookies_into_one_header() {
        let reply = serde_json::json!({"cookies": [{"name": "a", "value": "1"}, {"name": "b", "value": "2"}]});
        assert_eq!(super::cookie_header(&reply), "a=1; b=2");
        assert_eq!(super::cookie_header(&serde_json::json!({})), "");
    }
}
