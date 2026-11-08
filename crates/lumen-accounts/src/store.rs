use crate::crypto::{CryptoError, MasterKey};
use lumen_live::health::HealthSnapshot;
use lumen_live::{AccountId, Credentials, LiveTarget, Platform};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("account not found: {0}")]
    NotFound(String),
    #[error("ambiguous account reference '{0}' (matches several accounts)")]
    Ambiguous(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BroadcastSettings {
    /// The operator confirmed this account is authorized to receive our stream. Reading LIVE
    /// events never sets this.
    pub authorized: bool,
    pub enabled: bool,
    /// RTMP(S)/SRT ingest server URL (the stream key is stored encrypted as credential `stream_key`).
    pub ingest_url: Option<String>,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: AccountId,
    pub platform: Platform,
    pub label: String,
    pub handle: String,
    pub group: Option<String>,
    pub enabled: bool,
    pub live_enabled: bool,
    pub settings: BTreeMap<String, String>,
    pub broadcast: BroadcastSettings,
    /// Names of stored credential fields (values are never exposed here).
    pub credential_keys: Vec<String>,
    pub created_ms: i64,
    pub updated_ms: i64,
}

impl Account {
    pub fn live_target(&self) -> LiveTarget {
        LiveTarget { account_id: self.id.clone(), platform: self.platform, handle: self.handle.clone(), settings: self.settings.clone() }
    }
}

#[derive(Debug, Clone)]
pub struct NewAccount {
    pub platform: Platform,
    pub label: String,
    pub handle: String,
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub destinations: Vec<AccountId>,
}

pub struct AccountCenter {
    pool: SqlitePool,
    key: MasterKey,
}

const SCHEMA_VERSION: i64 = 1;

fn valid_label(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.chars().count() <= 64 && !t.chars().any(char::is_control)
}

pub fn valid_handle(platform: Platform, handle: &str) -> Result<String, StoreError> {
    let h = handle.trim().trim_start_matches('@');
    let ok_chars = |extra: &[u8]| h.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || extra.contains(&c));
    let ok = !h.is_empty()
        && h.len() <= 64
        && match platform {
            Platform::TikTok => ok_chars(&[]),
            Platform::KwaiGlobal => ok_chars(b"-"),
            Platform::LikeeGlobal => ok_chars(b"-"),
        };
    if ok { Ok(h.to_owned()) } else { Err(StoreError::Invalid(format!("invalid {platform} handle '{handle}'"))) }
}

pub fn valid_ingest_url(url: &str) -> Result<(), StoreError> {
    let lower = url.to_ascii_lowercase();
    let scheme_ok = ["rtmp://", "rtmps://", "srt://"].iter().any(|s| lower.starts_with(s));
    if !scheme_ok || url.len() > 512 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(StoreError::Invalid("ingest URL must be rtmp://, rtmps:// or srt:// without spaces".into()));
    }
    Ok(())
}

fn now_ms() -> i64 {
    lumen_live::now_ms()
}

impl AccountCenter {
    pub async fn open(path: &Path, key: MasterKey) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .pragma("secure_delete", "ON")
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        let center = Self { pool, key };
        center.migrate().await?;
        Ok(center)
    }

    pub async fn open_in_memory(key: MasterKey) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await?;
        let center = Self { pool, key };
        center.migrate().await?;
        Ok(center)
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&self.pool).await?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::Invalid(format!("database schema v{version} is newer than this build (v{SCHEMA_VERSION})")));
        }
        if version < 1 {
            sqlx::raw_sql(r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                platform TEXT NOT NULL CHECK(platform IN ('tiktok','kwai_global','likee_global')),
                label TEXT NOT NULL UNIQUE CHECK(length(label) BETWEEN 1 AND 256),
                handle TEXT NOT NULL CHECK(length(handle) BETWEEN 1 AND 64),
                group_name TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                live_enabled INTEGER NOT NULL DEFAULT 1,
                settings_json TEXT NOT NULL DEFAULT '{}',
                created_ms INTEGER NOT NULL,
                updated_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS accounts_group ON accounts(group_name);
            CREATE TABLE IF NOT EXISTS credentials (
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 64),
                key_id TEXT NOT NULL,
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL,
                updated_ms INTEGER NOT NULL,
                PRIMARY KEY(account_id, name)
            );
            CREATE TABLE IF NOT EXISTS broadcast_settings (
                account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                authorized INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 0,
                ingest_url TEXT,
                video_bitrate_kbps INTEGER NOT NULL DEFAULT 4500,
                audio_bitrate_kbps INTEGER NOT NULL DEFAULT 160
            );
            CREATE TABLE IF NOT EXISTS scenes (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE
            );
            CREATE TABLE IF NOT EXISTS scene_destinations (
                scene_id TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                PRIMARY KEY(scene_id, account_id)
            );
            CREATE TABLE IF NOT EXISTS account_metrics (
                account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                snapshot_json TEXT NOT NULL,
                updated_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            PRAGMA user_version = 1;
            "#).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn add(&self, new: NewAccount) -> Result<Account, StoreError> {
        if !valid_label(&new.label) {
            return Err(StoreError::Invalid("label must be 1-64 printable characters".into()));
        }
        let handle = valid_handle(new.platform, &new.handle)?;
        if let Some(g) = &new.group
            && !valid_label(g)
        {
            return Err(StoreError::Invalid("group must be 1-64 printable characters".into()));
        }
        let id = AccountId::generate();
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO accounts(id,platform,label,handle,group_name,created_ms,updated_ms) VALUES(?,?,?,?,?,?,?)")
            .bind(id.as_str()).bind(new.platform.as_str()).bind(new.label.trim()).bind(&handle).bind(new.group.as_deref().map(str::trim)).bind(now).bind(now)
            .execute(&mut *tx).await
            .map_err(|e| match &e { sqlx::Error::Database(d) if d.message().contains("UNIQUE") => StoreError::Invalid(format!("label '{}' already exists", new.label.trim())), _ => e.into() })?;
        sqlx::query("INSERT INTO broadcast_settings(account_id) VALUES(?)").bind(id.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::info!(account = %id.short(), platform = %new.platform, "account added");
        self.get(&id).await
    }

    pub async fn get(&self, id: &AccountId) -> Result<Account, StoreError> {
        let row = sqlx::query("SELECT a.*, b.authorized, b.enabled AS b_enabled, b.ingest_url, b.video_bitrate_kbps, b.audio_bitrate_kbps FROM accounts a LEFT JOIN broadcast_settings b ON b.account_id=a.id WHERE a.id=?")
            .bind(id.as_str()).fetch_optional(&self.pool).await?
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        self.row_to_account(row).await
    }

    async fn row_to_account(&self, row: sqlx::sqlite::SqliteRow) -> Result<Account, StoreError> {
        let id = AccountId(row.try_get("id")?);
        let platform: String = row.try_get("platform")?;
        let settings_json: String = row.try_get("settings_json")?;
        let credential_keys: Vec<String> = sqlx::query_scalar("SELECT name FROM credentials WHERE account_id=? ORDER BY name").bind(id.as_str()).fetch_all(&self.pool).await?;
        Ok(Account {
            platform: platform.parse().map_err(StoreError::Invalid)?,
            label: row.try_get("label")?,
            handle: row.try_get("handle")?,
            group: row.try_get("group_name")?,
            enabled: row.try_get::<i64, _>("enabled")? != 0,
            live_enabled: row.try_get::<i64, _>("live_enabled")? != 0,
            settings: serde_json::from_str(&settings_json).unwrap_or_default(),
            broadcast: BroadcastSettings {
                authorized: row.try_get::<Option<i64>, _>("authorized")?.unwrap_or(0) != 0,
                enabled: row.try_get::<Option<i64>, _>("b_enabled")?.unwrap_or(0) != 0,
                ingest_url: row.try_get("ingest_url")?,
                video_bitrate_kbps: row.try_get::<Option<i64>, _>("video_bitrate_kbps")?.unwrap_or(4500) as u32,
                audio_bitrate_kbps: row.try_get::<Option<i64>, _>("audio_bitrate_kbps")?.unwrap_or(160) as u32,
            },
            credential_keys,
            created_ms: row.try_get("created_ms")?,
            updated_ms: row.try_get("updated_ms")?,
            id,
        })
    }

    pub async fn list(&self) -> Result<Vec<Account>, StoreError> {
        let rows = sqlx::query("SELECT a.*, b.authorized, b.enabled AS b_enabled, b.ingest_url, b.video_bitrate_kbps, b.audio_bitrate_kbps FROM accounts a LEFT JOIN broadcast_settings b ON b.account_id=a.id ORDER BY a.group_name IS NULL, a.group_name, a.label")
            .fetch_all(&self.pool).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_account(row).await?);
        }
        Ok(out)
    }

    pub async fn list_group(&self, group: &str) -> Result<Vec<Account>, StoreError> {
        Ok(self.list().await?.into_iter().filter(|a| a.group.as_deref() == Some(group)).collect())
    }

    pub async fn groups(&self) -> Result<Vec<(String, i64)>, StoreError> {
        let rows = sqlx::query("SELECT group_name, COUNT(*) AS n FROM accounts WHERE group_name IS NOT NULL GROUP BY group_name ORDER BY group_name").fetch_all(&self.pool).await?;
        rows.into_iter().map(|r| Ok((r.try_get("group_name")?, r.try_get("n")?))).collect()
    }

    /// Resolve a full id, unique id prefix (≥4 chars) or exact label.
    pub async fn resolve(&self, reference: &str) -> Result<Account, StoreError> {
        let all = self.list().await?;
        if let Some(a) = all.iter().find(|a| a.id.as_str() == reference || a.label == reference) {
            return Ok(a.clone());
        }
        if reference.len() >= 4 {
            let matches: Vec<_> = all.iter().filter(|a| a.id.as_str().starts_with(reference)).collect();
            match matches.len() {
                1 => return Ok(matches[0].clone()),
                0 => {}
                _ => return Err(StoreError::Ambiguous(reference.into())),
            }
        }
        Err(StoreError::NotFound(reference.into()))
    }

    async fn touch(&self, id: &AccountId) -> Result<(), StoreError> {
        let n = sqlx::query("UPDATE accounts SET updated_ms=? WHERE id=?").bind(now_ms()).bind(id.as_str()).execute(&self.pool).await?.rows_affected();
        if n == 0 { Err(StoreError::NotFound(id.to_string())) } else { Ok(()) }
    }

    pub async fn set_group(&self, id: &AccountId, group: Option<&str>) -> Result<(), StoreError> {
        if let Some(g) = group
            && !valid_label(g)
        {
            return Err(StoreError::Invalid("group must be 1-64 printable characters".into()));
        }
        sqlx::query("UPDATE accounts SET group_name=? WHERE id=?").bind(group.map(str::trim)).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    pub async fn rename(&self, id: &AccountId, label: &str) -> Result<(), StoreError> {
        if !valid_label(label) {
            return Err(StoreError::Invalid("label must be 1-64 printable characters".into()));
        }
        sqlx::query("UPDATE accounts SET label=? WHERE id=?").bind(label.trim()).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    pub async fn set_enabled(&self, id: &AccountId, enabled: bool) -> Result<(), StoreError> {
        sqlx::query("UPDATE accounts SET enabled=? WHERE id=?").bind(enabled as i64).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    pub async fn set_live_enabled(&self, id: &AccountId, enabled: bool) -> Result<(), StoreError> {
        sqlx::query("UPDATE accounts SET live_enabled=? WHERE id=?").bind(enabled as i64).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    pub async fn set_setting(&self, id: &AccountId, key: &str, value: Option<&str>) -> Result<(), StoreError> {
        if key.is_empty() || key.len() > 64 || value.is_some_and(|v| v.len() > 1024) {
            return Err(StoreError::Invalid("setting key/value too long".into()));
        }
        let mut account = self.get(id).await?;
        match value {
            Some(v) => account.settings.insert(key.into(), v.into()),
            None => account.settings.remove(key),
        };
        let json = serde_json::to_string(&account.settings).map_err(|e| StoreError::Invalid(e.to_string()))?;
        sqlx::query("UPDATE accounts SET settings_json=? WHERE id=?").bind(json).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    /// Remove an account and (via cascade + secure_delete) its credentials, settings and metrics.
    pub async fn remove(&self, id: &AccountId) -> Result<(), StoreError> {
        let n = sqlx::query("DELETE FROM accounts WHERE id=?").bind(id.as_str()).execute(&self.pool).await?.rows_affected();
        if n == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        sqlx::query("DELETE FROM kv WHERE key='active_account' AND value=?").bind(id.as_str()).execute(&self.pool).await?;
        tracing::info!(account = %id.short(), "account removed");
        Ok(())
    }

    fn aad(account: &Account, name: &str) -> Vec<u8> {
        format!("lumen-credential-v1|{}|{}|{}", account.id, account.platform.as_str(), name).into_bytes()
    }

    pub async fn set_credential(&self, id: &AccountId, name: &str, value: &str) -> Result<(), StoreError> {
        if name.is_empty() || name.len() > 64 || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            return Err(StoreError::Invalid("credential name must be [A-Za-z0-9_]{1,64}".into()));
        }
        if value.len() > 16 * 1024 {
            return Err(StoreError::Invalid("credential value too large".into()));
        }
        let account = self.get(id).await?;
        let (nonce, ct) = self.key.seal(value.as_bytes(), &Self::aad(&account, name));
        sqlx::query("INSERT INTO credentials(account_id,name,key_id,nonce,ciphertext,updated_ms) VALUES(?,?,?,?,?,?) ON CONFLICT(account_id,name) DO UPDATE SET key_id=excluded.key_id, nonce=excluded.nonce, ciphertext=excluded.ciphertext, updated_ms=excluded.updated_ms")
            .bind(id.as_str()).bind(name).bind(self.key.id()).bind(nonce).bind(ct).bind(now_ms())
            .execute(&self.pool).await?;
        tracing::info!(account = %id.short(), credential = name, "credential stored (encrypted)");
        self.touch(id).await
    }

    pub async fn delete_credential(&self, id: &AccountId, name: &str) -> Result<bool, StoreError> {
        let n = sqlx::query("DELETE FROM credentials WHERE account_id=? AND name=?").bind(id.as_str()).bind(name).execute(&self.pool).await?.rows_affected();
        Ok(n > 0)
    }

    /// Decrypt this account's credentials. Each account only ever sees its own.
    pub async fn credentials(&self, id: &AccountId) -> Result<Credentials, StoreError> {
        let account = self.get(id).await?;
        let rows = sqlx::query("SELECT name,key_id,nonce,ciphertext FROM credentials WHERE account_id=?").bind(id.as_str()).fetch_all(&self.pool).await?;
        let mut creds = Credentials::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let key_id: String = row.try_get("key_id")?;
            let nonce: Vec<u8> = row.try_get("nonce")?;
            let ct: Vec<u8> = row.try_get("ciphertext")?;
            let plain = self.key.open(&key_id, &nonce, &ct, &Self::aad(&account, &name))?;
            let text = std::str::from_utf8(&plain).map_err(|_| StoreError::Crypto(CryptoError::Decrypt))?;
            creds.set(&name, text);
        }
        Ok(creds)
    }

    pub async fn set_broadcast(&self, id: &AccountId, ingest_url: Option<&str>, video_kbps: Option<u32>, audio_kbps: Option<u32>) -> Result<(), StoreError> {
        if let Some(url) = ingest_url {
            valid_ingest_url(url)?;
        }
        if video_kbps.is_some_and(|v| !(300..=20_000).contains(&v)) || audio_kbps.is_some_and(|a| !(32..=512).contains(&a)) {
            return Err(StoreError::Invalid("bitrate out of range (video 300-20000 kbps, audio 32-512 kbps)".into()));
        }
        let current = self.get(id).await?.broadcast;
        sqlx::query("UPDATE broadcast_settings SET ingest_url=?, video_bitrate_kbps=?, audio_bitrate_kbps=? WHERE account_id=?")
            .bind(ingest_url.map(str::to_owned).or(current.ingest_url))
            .bind(video_kbps.unwrap_or(current.video_bitrate_kbps) as i64)
            .bind(audio_kbps.unwrap_or(current.audio_bitrate_kbps) as i64)
            .bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    /// Record the operator's explicit confirmation that this account may receive our stream.
    pub async fn set_broadcast_authorized(&self, id: &AccountId, authorized: bool) -> Result<(), StoreError> {
        sqlx::query("UPDATE broadcast_settings SET authorized=?, enabled=CASE WHEN ? THEN enabled ELSE 0 END WHERE account_id=?")
            .bind(authorized as i64).bind(authorized as i64).bind(id.as_str()).execute(&self.pool).await?;
        tracing::info!(account = %id.short(), authorized, "broadcast authorization changed");
        self.touch(id).await
    }

    pub async fn set_broadcast_enabled(&self, id: &AccountId, enabled: bool) -> Result<(), StoreError> {
        let account = self.get(id).await?;
        if enabled && !account.broadcast.authorized {
            return Err(StoreError::Invalid(format!("account '{}' is not authorized for broadcasting; authorize it first", account.label)));
        }
        sqlx::query("UPDATE broadcast_settings SET enabled=? WHERE account_id=?").bind(enabled as i64).bind(id.as_str()).execute(&self.pool).await?;
        self.touch(id).await
    }

    pub async fn set_active(&self, id: &AccountId) -> Result<(), StoreError> {
        self.get(id).await?;
        sqlx::query("INSERT INTO kv(key,value) VALUES('active_account',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(id.as_str()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn active(&self) -> Result<Option<Account>, StoreError> {
        let id: Option<String> = sqlx::query_scalar("SELECT value FROM kv WHERE key='active_account'").fetch_optional(&self.pool).await?;
        match id {
            Some(id) => self.get(&AccountId(id)).await.map(Some).or_else(|e| match e { StoreError::NotFound(_) => Ok(None), e => Err(e) }),
            None => Ok(None),
        }
    }

    pub async fn upsert_scene(&self, name: &str, destinations: &[AccountId]) -> Result<Scene, StoreError> {
        if !valid_label(name) {
            return Err(StoreError::Invalid("scene name must be 1-64 printable characters".into()));
        }
        let mut tx = self.pool.begin().await?;
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM scenes WHERE name=?").bind(name).fetch_optional(&mut *tx).await?;
        let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        sqlx::query("INSERT OR IGNORE INTO scenes(id,name) VALUES(?,?)").bind(&id).bind(name).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM scene_destinations WHERE scene_id=?").bind(&id).execute(&mut *tx).await?;
        for d in destinations {
            sqlx::query("INSERT INTO scene_destinations(scene_id,account_id) VALUES(?,?)").bind(&id).bind(d.as_str()).execute(&mut *tx).await
                .map_err(|e| match &e { sqlx::Error::Database(db) if db.message().contains("FOREIGN KEY") => StoreError::NotFound(d.to_string()), _ => e.into() })?;
        }
        tx.commit().await?;
        self.scene(name).await
    }

    pub async fn scene(&self, name: &str) -> Result<Scene, StoreError> {
        let id: String = sqlx::query_scalar("SELECT id FROM scenes WHERE name=?").bind(name).fetch_optional(&self.pool).await?.ok_or_else(|| StoreError::NotFound(format!("scene {name}")))?;
        let destinations = sqlx::query_scalar::<_, String>("SELECT account_id FROM scene_destinations WHERE scene_id=? ORDER BY account_id").bind(&id).fetch_all(&self.pool).await?.into_iter().map(AccountId).collect();
        Ok(Scene { id, name: name.into(), destinations })
    }

    pub async fn scenes(&self) -> Result<Vec<Scene>, StoreError> {
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM scenes ORDER BY name").fetch_all(&self.pool).await?;
        let mut out = Vec::new();
        for n in names {
            out.push(self.scene(&n).await?);
        }
        Ok(out)
    }

    pub async fn record_metrics(&self, snapshot: &HealthSnapshot) -> Result<(), StoreError> {
        let json = serde_json::to_string(snapshot).map_err(|e| StoreError::Invalid(e.to_string()))?;
        sqlx::query("INSERT INTO account_metrics(account_id,snapshot_json,updated_ms) VALUES(?,?,?) ON CONFLICT(account_id) DO UPDATE SET snapshot_json=excluded.snapshot_json, updated_ms=excluded.updated_ms")
            .bind(snapshot.account_id.as_str()).bind(json).bind(now_ms()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn metrics(&self, id: &AccountId) -> Result<Option<serde_json::Value>, StoreError> {
        let s: Option<String> = sqlx::query_scalar("SELECT snapshot_json FROM account_metrics WHERE account_id=?").bind(id.as_str()).fetch_optional(&self.pool).await?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok()))
    }
}
