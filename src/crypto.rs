use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::canonical::CanonicalEncoder;

pub type HashBytes = [u8; 32];

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("некорректная шестнадцатеричная строка: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("ожидалось {expected} байт, получено {actual}")]
    Length { expected: usize, actual: usize },
    #[error("некорректный открытый ключ Ed25519")]
    PublicKey,
    #[error("некорректная подпись Ed25519")]
    Signature,
}

pub fn sha256(input: &[u8]) -> HashBytes {
    Sha256::digest(input).into()
}

pub fn sha256_hex(input: &[u8]) -> String {
    hex::encode(sha256(input))
}

pub fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N], CryptoError> {
    let decoded = hex::decode(value)?;
    let actual = decoded.len();
    decoded.try_into().map_err(|_| CryptoError::Length {
        expected: N,
        actual,
    })
}

pub fn public_key_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_bytes())
}

pub fn sign_hex(key: &SigningKey, message: &[u8]) -> String {
    hex::encode(key.sign(message).to_bytes())
}

pub fn verify_hex(public_key: &str, message: &[u8], signature: &str) -> Result<(), CryptoError> {
    let public_bytes = decode_fixed::<32>(public_key)?;
    let signature_bytes = decode_fixed::<64>(signature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_bytes).map_err(|_| CryptoError::PublicKey)?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify(message, &signature)
        .map_err(|_| CryptoError::Signature)
}

pub fn merkle_root_from_ids(ids: &[String]) -> Result<String, CryptoError> {
    if ids.is_empty() {
        return Ok(sha256_hex(b"RRQ/MERKLE/EMPTY/V1"));
    }

    let mut level = ids
        .iter()
        .map(|id| {
            let id = decode_fixed::<32>(id)?;
            let mut encoder = CanonicalEncoder::new("RRQ/MERKLE/LEAF");
            encoder.put_fixed(&id);
            Ok(sha256(&encoder.finish()))
        })
        .collect::<Result<Vec<_>, CryptoError>>()?;

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let left = pair[0];
            let right = *pair.get(1).unwrap_or(&left);
            let mut encoder = CanonicalEncoder::new("RRQ/MERKLE/NODE");
            encoder.put_fixed(&left);
            encoder.put_fixed(&right);
            next.push(sha256(&encoder.finish()));
        }
        level = next;
    }

    Ok(hex::encode(level[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_deterministic() {
        let first = sha256_hex(b"round-robin-quorum");
        let second = sha256_hex(b"round-robin-quorum");
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn ed25519_signature_round_trip_and_tamper_detection() {
        let key = SigningKey::from_bytes(&[42; 32]);
        let signature = sign_hex(&key, b"message");
        let public_key = public_key_hex(&key);
        assert!(verify_hex(&public_key, b"message", &signature).is_ok());
        assert!(verify_hex(&public_key, b"changed", &signature).is_err());
    }

    #[test]
    fn merkle_root_handles_empty_single_and_odd_levels() {
        let a = sha256_hex(b"a");
        let b = sha256_hex(b"b");
        let c = sha256_hex(b"c");
        assert_eq!(
            merkle_root_from_ids(&[]).unwrap(),
            sha256_hex(b"RRQ/MERKLE/EMPTY/V1")
        );
        assert_ne!(merkle_root_from_ids(std::slice::from_ref(&a)).unwrap(), a);
        assert_ne!(
            merkle_root_from_ids(&[a.clone(), b.clone(), c.clone()]).unwrap(),
            merkle_root_from_ids(&[a, c, b]).unwrap()
        );
    }
}
