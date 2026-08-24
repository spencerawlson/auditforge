//! Password hashing and the password policy.
//!
//! Argon2id with the crate's default parameters (19 MiB, t=2, p=1), which is the OWASP
//! baseline at time of writing. Parameters are encoded in the stored PHC string, so
//! raising them later re-hashes on next login rather than invalidating accounts.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use subtle::ConstantTimeEq;

use crate::error::{Error, Result};

/// Below this, a passphrase is not worth the Argon2 cycles.
pub const MIN_PASSWORD_LENGTH: usize = 12;
/// Guard against a multi-megabyte "password" being fed to the hasher.
pub const MAX_PASSWORD_LENGTH: usize = 1024;

/// Rejected outright regardless of length. Deliberately short: this is a speed bump for
/// obvious choices, not a substitute for a breach corpus.
const OBVIOUS_PASSWORDS: &[&str] = &[
    "password",
    "password1",
    "password123",
    "passw0rd",
    "123456789012",
    "1234567890123",
    "qwertyuiop123",
    "letmein12345",
    "administrator",
    "changeme1234",
    "welcome12345",
    "iloveyou1234",
    "auditforge123",
];

fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::DEFAULT)
}

/// Hash a password for storage. The returned PHC string embeds algorithm, parameters
/// and a fresh random salt.
pub fn hash_password(password: &str) -> Result<String> {
    validate_password_strength(password)?;
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| Error::internal(format!("password hashing failed: {e}")))
}

/// Verify a password against a stored PHC string.
///
/// A malformed stored hash is treated as a failed verification rather than an error, so
/// a corrupted row cannot be probed to distinguish "no such user" from "bad hash".
pub fn verify_password(password: &str, stored_phc: &str) -> bool {
    if password.len() > MAX_PASSWORD_LENGTH {
        return false;
    }
    match PasswordHash::new(stored_phc) {
        Ok(parsed) => argon2()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Enforce the password policy. Returns the reason on rejection so the sign-up form can
/// show something actionable.
pub fn validate_password_strength(password: &str) -> Result<()> {
    let length = password.chars().count();
    if length < MIN_PASSWORD_LENGTH {
        return Err(Error::validation(format!(
            "password must be at least {MIN_PASSWORD_LENGTH} characters"
        )));
    }
    if password.len() > MAX_PASSWORD_LENGTH {
        return Err(Error::validation("password is too long"));
    }
    if password.trim().is_empty() {
        return Err(Error::validation("password cannot be only whitespace"));
    }

    let lowered = password.to_lowercase();
    if OBVIOUS_PASSWORDS
        .iter()
        .any(|candidate| lowered == *candidate)
    {
        return Err(Error::validation("that password is too easy to guess"));
    }

    let classes = [
        password.chars().any(|c| c.is_lowercase()),
        password.chars().any(|c| c.is_uppercase()),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|present| **present)
    .count();

    if classes < 3 {
        return Err(Error::validation(
            "password must combine at least three of: lowercase, uppercase, digits, symbols",
        ));
    }

    Ok(())
}

/// A rough 0-4 strength score for the sign-up meter. Advisory only; acceptance is
/// decided by `validate_password_strength`.
pub fn strength_score(password: &str) -> u8 {
    let length = password.chars().count();
    let classes = [
        password.chars().any(|c| c.is_lowercase()),
        password.chars().any(|c| c.is_uppercase()),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|present| **present)
    .count();

    let mut score = 0u8;
    if length >= MIN_PASSWORD_LENGTH {
        score += 1;
    }
    if length >= 16 {
        score += 1;
    }
    if classes >= 3 {
        score += 1;
    }
    if classes == 4 && length >= 20 {
        score += 1;
    }
    score
}

/// Constant-time comparison for secrets that are compared as bytes (session token
/// hashes, evidence digests). Prevents an attacker from learning a prefix by timing.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// Generate `len` cryptographically random bytes.
pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "Correct-Horse-9-Battery";

    #[test]
    fn hashes_are_argon2id_and_salted() {
        let first = hash_password(GOOD).unwrap();
        let second = hash_password(GOOD).unwrap();
        assert!(first.starts_with("$argon2id$"), "got {first}");
        assert_ne!(first, second, "each hash must use a fresh salt");
        assert!(!first.contains(GOOD), "hash must not contain the password");
    }

    #[test]
    fn verification_accepts_the_password_and_rejects_others() {
        let hash = hash_password(GOOD).unwrap();
        assert!(verify_password(GOOD, &hash));
        assert!(!verify_password("Correct-Horse-9-Batteryx", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn verification_of_a_corrupt_hash_fails_closed() {
        assert!(!verify_password(GOOD, "not-a-phc-string"));
        assert!(!verify_password(GOOD, ""));
        assert!(!verify_password(
            GOOD,
            "$argon2id$v=19$m=1,t=1,p=1$aaaa$bbbb"
        ));
    }

    #[test]
    fn policy_rejects_short_common_and_low_variety_passwords() {
        assert!(validate_password_strength("short1A!").is_err());
        assert!(validate_password_strength("password123").is_err());
        assert!(validate_password_strength("aaaaaaaaaaaaaaaaaaaa").is_err());
        assert!(validate_password_strength("            ").is_err());
        assert!(validate_password_strength(&"a".repeat(MAX_PASSWORD_LENGTH + 1)).is_err());
        assert!(validate_password_strength(GOOD).is_ok());
    }

    #[test]
    fn oversized_password_never_reaches_the_hasher() {
        let hash = hash_password(GOOD).unwrap();
        let huge = "a".repeat(MAX_PASSWORD_LENGTH + 1);
        assert!(!verify_password(&huge, &hash));
    }

    #[test]
    fn strength_score_increases_with_length_and_variety() {
        assert_eq!(strength_score("aaaa"), 0);
        assert!(strength_score(GOOD) >= 2);
        assert!(strength_score("Correct-Horse-9-Battery-Staple!") > strength_score("Abcdefgh123!"));
    }

    #[test]
    fn constant_time_eq_matches_normal_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn random_bytes_are_not_constant() {
        let a = random_bytes(32);
        let b = random_bytes(32);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
