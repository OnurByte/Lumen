use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported platforms. `KwaiGlobal` is the international Kwai app (kwai.com /
/// kwai-pro.com), **not** the Chinese Kuaishou service; the two have separate APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    #[serde(rename = "tiktok")]
    TikTok,
    KwaiGlobal,
    LikeeGlobal,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::TikTok, Platform::KwaiGlobal, Platform::LikeeGlobal];
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::TikTok => "tiktok",
            Platform::KwaiGlobal => "kwai_global",
            Platform::LikeeGlobal => "likee_global",
        }
    }
    /// Short prefix used in compact event ids.
    pub fn short(self) -> &'static str {
        match self {
            Platform::TikTok => "tt",
            Platform::KwaiGlobal => "kw",
            Platform::LikeeGlobal => "lk",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Platform {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "tiktok" | "tt" => Ok(Platform::TikTok),
            "kwai" | "kwai_global" | "kw" => Ok(Platform::KwaiGlobal),
            "likee" | "likee_global" | "lk" => Ok(Platform::LikeeGlobal),
            "kuaishou" => Err("Chinese Kuaishou is not supported; use kwai_global for the international Kwai app".into()),
            other => Err(format!("unknown platform '{other}' (expected tiktok, kwai_global, likee_global)")),
        }
    }
}

/// Stable account identifier (UUID string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// First 8 characters, for compact identifiers and logs.
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(8)]
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How far a capability has been validated. Never report `Verified` without a
/// successful run against the real service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// Exercised against the live service.
    Verified,
    /// Implemented and fixture-tested, but not confirmed against the live service.
    Experimental,
    /// Not implemented / no known working protocol.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub profile: Support,
    pub search: Support,
    pub videos: Support,
    pub trending: Support,
    pub live_events: Support,
    pub chat: Support,
    pub gift: Support,
    pub like: Support,
    pub join: Support,
    pub follow: Support,
    pub share: Support,
    pub viewer_count: Support,
    /// Pushing video to the platform via RTMP(S) using a user-supplied, authorized ingest key.
    pub broadcast_rtmp: Support,
    /// Obtaining the ingest URL/key (and starting/stopping the LIVE) through the platform itself.
    pub broadcast_provision: Support,
}

impl Capabilities {
    pub const NONE: Capabilities = Capabilities {
        profile: Support::Unsupported,
        search: Support::Unsupported,
        videos: Support::Unsupported,
        trending: Support::Unsupported,
        live_events: Support::Unsupported,
        chat: Support::Unsupported,
        gift: Support::Unsupported,
        like: Support::Unsupported,
        join: Support::Unsupported,
        follow: Support::Unsupported,
        share: Support::Unsupported,
        viewer_count: Support::Unsupported,
        broadcast_rtmp: Support::Unsupported,
        broadcast_provision: Support::Unsupported,
    };
    pub fn rows(&self) -> [(&'static str, Support); 14] {
        [
            ("profile", self.profile),
            ("search", self.search),
            ("videos", self.videos),
            ("trending", self.trending),
            ("live_events", self.live_events),
            ("chat", self.chat),
            ("gift", self.gift),
            ("like", self.like),
            ("join", self.join),
            ("follow", self.follow),
            ("share", self.share),
            ("viewer_count", self.viewer_count),
            ("broadcast_rtmp", self.broadcast_rtmp),
            ("broadcast_provision", self.broadcast_provision),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kuaishou_is_rejected_not_aliased_to_kwai() {
        assert!("kuaishou".parse::<Platform>().is_err());
        assert_eq!("kwai".parse::<Platform>().unwrap(), Platform::KwaiGlobal);
        for p in Platform::ALL {
            assert_eq!(p.as_str().parse::<Platform>().unwrap(), p);
        }
    }
}
