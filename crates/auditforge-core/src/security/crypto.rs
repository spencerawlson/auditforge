//! Authenticated encryption for evidence at rest.
//!
//! AES-256-GCM with a random 96-bit nonce per file. The stored blob is
//! `magic || version || nonce || ciphertext||tag`, so a file can be identified as
//! AuditForge-encrypted without consulting the database, and any tampering fails the
//! GCM tag check on decryption rather than yielding garbage plaintext.
//!
//! The vault key is 32 random bytes held in the OS keychain (see `security::secrets`).
//! It is never written to the database.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::auth::password::random_bytes;
use crate::error::{Error, Result};

const MAGIC: &[u8; 4] = b"AFE1";
const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;

/// A 256-bit symmetric key, wiped from memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct VaultKey([u8; KEY_LEN]);

impl VaultKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        bytes.copy_from_slice(&random_bytes(KEY_LEN));
        VaultKey(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_LEN {
            return Err(Error::internal("vault key must be 32 bytes"));
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(bytes);
        Ok(VaultKey(key))
    }

    /// Encode for storage in the OS keychain, which holds strings.
    pub fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0)
    }

    pub fn from_base64(encoded: &str) -> Result<Self> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|_| Error::internal("stored vault key is not valid base64"))?;
        Self::from_bytes(&bytes)
    }
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material, not even in a panic backtrace.
        f.write_str("VaultKey(redacted)")
    }
}

/// Encrypt `plaintext`. `associated_data` is authenticated but not encrypted; the
/// evidence vault passes the evidence reference so a blob cannot be swapped between
/// records without detection.
pub fn encrypt(key: &VaultKey, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));
    let nonce_bytes = random_bytes(NONCE_LEN);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|_| Error::internal("evidence encryption failed"))?;

    let mut out = Vec::with_capacity(MAGIC.len() + 1 + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt(key: &VaultKey, blob: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
    let header = MAGIC.len() + 1;
    if blob.len() < header + NONCE_LEN + 16 || &blob[..MAGIC.len()] != MAGIC {
        return Err(Error::validation(
            "that file is not AuditForge-encrypted evidence",
        ));
    }
    if blob[MAGIC.len()] != VERSION {
        return Err(Error::validation("unsupported evidence encryption version"));
    }
    let nonce = Nonce::from_slice(&blob[header..header + NONCE_LEN]);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &blob[header + NONCE_LEN..],
                aad: associated_data,
            },
        )
        .map_err(|_| {
            Error::validation("evidence could not be decrypted; it may have been modified")
        })
}

/// True when a blob carries the AuditForge encryption header.
pub fn is_encrypted_blob(blob: &[u8]) -> bool {
    blob.len() > MAGIC.len() && &blob[..MAGIC.len()] == MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovers_the_plaintext() {
        let key = VaultKey::generate();
        let plaintext = b"Nmap scan report for 10.0.0.5\nPort 22/tcp open";
        let blob = encrypt(&key, plaintext, b"EVD-2026-0001").unwrap();
        assert!(is_encrypted_blob(&blob));
        assert_ne!(&blob[..], &plaintext[..]);
        let recovered = decrypt(&key, &blob, b"EVD-2026-0001").unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn each_encryption_uses_a_fresh_nonce() {
        let key = VaultKey::generate();
        let a = encrypt(&key, b"same input", b"aad").unwrap();
        let b = encrypt(&key, b"same input", b"aad").unwrap();
        assert_ne!(
            a, b,
            "identical plaintext must not produce identical ciphertext"
        );
    }

    #[test]
    fn tampering_is_detected() {
        let key = VaultKey::generate();
        let mut blob = encrypt(&key, b"evidence bytes", b"EVD-1").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(decrypt(&key, &blob, b"EVD-1").is_err());
    }

    #[test]
    fn a_blob_cannot_be_moved_to_another_evidence_record() {
        let key = VaultKey::generate();
        let blob = encrypt(&key, b"evidence bytes", b"EVD-1").unwrap();
        assert!(decrypt(&key, &blob, b"EVD-2").is_err());
    }

    #[test]
    fn the_wrong_key_fails_closed() {
        let blob = encrypt(&VaultKey::generate(), b"secret", b"aad").unwrap();
        assert!(decrypt(&VaultKey::generate(), &blob, b"aad").is_err());
    }

    #[test]
    fn malformed_input_is_rejected_without_panicking() {
        let key = VaultKey::generate();
        assert!(decrypt(&key, b"", b"").is_err());
        assert!(decrypt(&key, b"AFE1", b"").is_err());
        assert!(decrypt(&key, b"not an auditforge blob at all really", b"").is_err());
    }

    #[test]
    fn keys_round_trip_through_base64_and_never_print() {
        let key = VaultKey::generate();
        let encoded = key.to_base64();
        let restored = VaultKey::from_base64(&encoded).unwrap();
        let blob = encrypt(&key, b"x", b"a").unwrap();
        assert_eq!(decrypt(&restored, &blob, b"a").unwrap(), b"x");
        assert_eq!(format!("{key:?}"), "VaultKey(redacted)");
        assert!(VaultKey::from_base64("not base64!!").is_err());
        assert!(VaultKey::from_bytes(b"too short").is_err());
    }
}
