//! Utilita' SHA-256 con output esadecimale minuscolo.

use sha2::{Digest, Sha256};
use std::fmt::Write;

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            // Scrivere su una String non puo' fallire.
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hex_is_zero_padded() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }
}
