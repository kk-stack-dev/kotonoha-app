//! Pairing two computers: both screens show the same six-digit code, and the person confirms it on the
//! computer that accepts work.
//!
//! The code is derived from the encrypted channel's handshake hash, so a computer in the middle that
//! runs two separate channels sees two different hashes. On its own a 6-digit code could be matched by
//! trying many handshakes, so the code also mixes in two random nonces exchanged commit-first: the
//! requesting computer commits to its nonce before it sees the other one, and the accepting computer
//! sends its nonce before it learns the first. A computer in the middle then has to fix one side's code
//! before it knows the other, and matches it only by a one-in-a-million chance.

use std::fmt;

use sha2::{Digest, Sha256};

pub type Nonce = [u8; 32];

fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u32).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// What the requesting computer sends before it sees the other nonce.
pub fn commit(nonce: &Nonce) -> [u8; 32] {
    hash(&[b"kotonoha-pair-commit", nonce])
}

/// The revealed nonce matches the earlier commitment.
pub fn verify(commitment: &[u8; 32], nonce: &Nonce) -> bool {
    commit(nonce) == *commitment
}

/// The six digits both screens show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingCode(u32);

impl PairingCode {
    pub fn new(handshake_hash: &[u8], client_nonce: &Nonce, server_nonce: &Nonce) -> Self {
        let digest = hash(&[b"kotonoha-pair-code", handshake_hash, client_nonce, server_nonce]);
        let n = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"));
        Self((n % 1_000_000) as u32)
    }
}

/// "042 917": two groups of three, easy to compare at a glance.
impl fmt::Display for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:03} {:03}", self.0 / 1000, self.0 % 1000)
    }
}

/// A short, stable name for a computer's public key, used to find it again on the LAN.
pub fn fingerprint(public_key: &[u8]) -> String {
    hex(&hash(&[b"kotonoha-id", public_key])[..8])
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_binds_the_nonce() {
        let a = [1; 32];
        let c = commit(&a);
        assert!(verify(&c, &a));
        assert!(!verify(&c, &[2; 32]));
    }

    #[test]
    fn code_depends_on_every_input() {
        let base = PairingCode::new(b"hash", &[1; 32], &[2; 32]);
        assert_eq!(base, PairingCode::new(b"hash", &[1; 32], &[2; 32]));
        assert_ne!(base, PairingCode::new(b"hasi", &[1; 32], &[2; 32]));
        assert_ne!(base, PairingCode::new(b"hash", &[3; 32], &[2; 32]));
        assert_ne!(base, PairingCode::new(b"hash", &[1; 32], &[3; 32]));
        // The two nonces are not interchangeable
        assert_ne!(base, PairingCode::new(b"hash", &[2; 32], &[1; 32]));
    }

    #[test]
    fn code_shows_six_digits_with_leading_zeros() {
        assert_eq!(PairingCode(42_917).to_string(), "042 917");
        assert_eq!(PairingCode(7).to_string(), "000 007");
        assert_eq!(PairingCode(999_999).to_string(), "999 999");
    }

    #[test]
    fn hex_round_trip_and_fingerprint() {
        let bytes = [0xab; 32];
        assert_eq!(from_hex::<32>(&hex(&bytes)), Some(bytes));
        assert_eq!(from_hex::<32>("zz"), None);
        assert_eq!(from_hex::<2>("abcd"), Some([0xab, 0xcd]));
        assert_eq!(from_hex::<2>("abc"), None);
        assert_eq!(fingerprint(&[1; 32]).len(), 16);
        assert_ne!(fingerprint(&[1; 32]), fingerprint(&[2; 32]));
    }
}
