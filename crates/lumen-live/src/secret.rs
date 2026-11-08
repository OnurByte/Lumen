//! Secrets that never print, never serialize implicitly and are wiped on drop.
use std::collections::BTreeMap;
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Default, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Explicit, greppable access to the secret material.
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([redacted])")
    }
}
impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Per-account credential bag (e.g. `cookie`, `stream_key`, `device_id`).
/// Keys are public names; values are secrets. Debug output lists keys only.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    values: BTreeMap<String, SecretString>,
}

impl Credentials {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.set(key, value);
        self
    }
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_owned(), SecretString::new(value));
    }
    pub fn get(&self, key: &str) -> Option<&SecretString> {
        self.values.get(key).filter(|v| !v.is_empty())
    }
    pub fn remove(&mut self, key: &str) -> Option<SecretString> {
        self.values.remove(key)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    /// Serialization used only by the encrypted credential store.
    pub fn to_plaintext_json(&self) -> String {
        let map: BTreeMap<&str, &str> = self
            .values
            .iter()
            .map(|(k, v)| (k.as_str(), v.expose()))
            .collect();
        serde_json::to_string(&map).unwrap_or_else(|_| "{}".into())
    }
    pub fn from_plaintext_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut map: BTreeMap<String, String> = serde_json::from_str(json)?;
        let values = std::mem::take(&mut map)
            .into_iter()
            .map(|(k, v)| (k, SecretString::new(v)))
            .collect();
        Ok(Self { values })
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("keys", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn debug_never_leaks_secret_values() {
        let c = Credentials::new().with("cookie", "sessionid=TOPSECRET");
        let shown = format!("{c:?} {:?} {}", c.get("cookie").unwrap(), c.get("cookie").unwrap());
        assert!(!shown.contains("TOPSECRET"));
        assert!(shown.contains("cookie"));
        let round = Credentials::from_plaintext_json(&c.to_plaintext_json()).unwrap();
        assert_eq!(round.get("cookie").unwrap().expose(), "sessionid=TOPSECRET");
    }
}
