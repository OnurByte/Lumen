//! Credential encryption: XChaCha20-Poly1305 with a 256-bit master key kept in a 0600 file.
//! Each ciphertext is bound to its account id and platform through AEAD associated data,
//! so encrypted blobs cannot be swapped between accounts.
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("master key io: {0}")]
    Io(#[from] std::io::Error),
    #[error("master key file must be exactly 32 bytes")]
    BadKeyLength,
    #[error("master key file permissions are too open (expected 0600)")]
    InsecurePermissions,
    #[error("credential was encrypted with a different master key")]
    WrongKey,
    #[error("credential decryption failed (corrupted or tampered)")]
    Decrypt,
}

pub struct MasterKey {
    key: Zeroizing<[u8; 32]>,
    id: String,
}

impl MasterKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        let digest = Sha256::digest(bytes);
        let id = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
        Self { key: Zeroizing::new(bytes), id }
    }

    /// Load the key from `path`, creating a fresh random key (mode 0600) if missing.
    pub fn load_or_create(path: &Path) -> Result<Self, CryptoError> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        match fs::metadata(path) {
            Ok(meta) => {
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err(CryptoError::InsecurePermissions);
                }
                let bytes = Zeroizing::new(fs::read(path)?);
                let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| CryptoError::BadKeyLength)?;
                Ok(Self::from_bytes(arr))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut bytes = Zeroizing::new([0u8; 32]);
                rand::fill(&mut bytes[..]);
                let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
                file.write_all(&bytes[..])?;
                file.sync_all()?;
                Ok(Self::from_bytes(*bytes))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Short public fingerprint stored next to ciphertexts to detect key mix-ups.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        let mut nonce = [0u8; 24];
        rand::fill(&mut nonce);
        let ct = cipher
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad })
            .expect("XChaCha20-Poly1305 encryption is infallible for in-memory buffers");
        (nonce.to_vec(), ct)
    }

    pub fn open(&self, key_id: &str, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        if key_id != self.id {
            return Err(CryptoError::WrongKey);
        }
        if nonce.len() != 24 {
            return Err(CryptoError::Decrypt);
        }
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        cipher
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ciphertext, aad })
            .map(Zeroizing::new)
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_aad_binding() {
        let k = MasterKey::from_bytes([7; 32]);
        let (n, c) = k.seal(b"cookie=secret", b"acct-1|tiktok");
        assert_eq!(&k.open(k.id(), &n, &c, b"acct-1|tiktok").unwrap()[..], b"cookie=secret");
        assert!(k.open(k.id(), &n, &c, b"acct-2|tiktok").is_err());
        assert!(!c.windows(6).any(|w| w == b"secret"));
        let other = MasterKey::from_bytes([8; 32]);
        assert!(matches!(other.open(k.id(), &n, &c, b"acct-1|tiktok"), Err(CryptoError::WrongKey)));
    }

    #[test]
    fn key_file_created_private_and_reloaded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys/master.key");
        let a = MasterKey::load_or_create(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let b = MasterKey::load_or_create(&path).unwrap();
        assert_eq!(a.id(), b.id());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(MasterKey::load_or_create(&path), Err(CryptoError::InsecurePermissions)));
    }
}
