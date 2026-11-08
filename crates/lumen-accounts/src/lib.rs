//! Account Center: unlimited TikTok, Kwai Global and Likee Global accounts, each with
//! isolated (encrypted) credentials, LIVE-read settings, broadcast settings and metrics.
pub mod crypto;
pub mod store;

pub use crypto::{CryptoError, MasterKey};
pub use store::{Account, AccountCenter, BroadcastSettings, NewAccount, Scene, StoreError};

/// `$LUMEN_HOME`, else `$XDG_DATA_HOME/lumen`, else `~/.local/share/lumen`.
pub fn default_home() -> std::path::PathBuf {
    std::env::var_os("LUMEN_HOME").map(Into::into).unwrap_or_else(|| {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| ".".into())
            .join("lumen")
    })
}

/// Open the user's Account Center (creating the master key on first use).
pub async fn open_default() -> Result<AccountCenter, String> {
    open_at(&default_home()).await
}

/// Open the Account Center stored under `dir` (`accounts.sqlite` + `keys/master.key`).
pub async fn open_at(dir: &std::path::Path) -> Result<AccountCenter, String> {
    let key = MasterKey::load_or_create(&dir.join("keys/master.key")).map_err(|e| e.to_string())?;
    AccountCenter::open(&dir.join("accounts.sqlite"), key).await.map_err(|e| e.to_string())
}
