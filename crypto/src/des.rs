use cbc::{Decryptor as CbcDecryptor, Encryptor as CbcEncryptor};
use cipher::{
    block_padding::Pkcs7,
    BlockDecryptMut, BlockEncryptMut, KeyIvInit,
};
use des::Des;

pub const DES_IV: [u8; 8] = [111, 151, 50, 205, 123, 222, 185, 45];
const DES_BLOCK_SIZE: usize = 8;

/// Errors returned by DES/CBC helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesError {
    InvalidKeyLength { minimum: usize, actual: usize },
    InvalidBlockLength,
    InvalidPadding,
}

impl std::fmt::Display for DesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKeyLength { minimum, actual } => {
                write!(f, "invalid DES key length: expected at least {minimum} bytes, got {actual}")
            }
            Self::InvalidBlockLength => write!(f, "DES ciphertext length is not a multiple of 8 bytes"),
            Self::InvalidPadding => write!(f, "invalid DES padding"),
        }
    }
}

impl std::error::Error for DesError {}

/// DES/CBC/PKCS#7 encryption. The protocol uses the first eight key bytes.
pub fn try_encrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, DesError> {
    if key.len() < DES_BLOCK_SIZE {
        return Err(DesError::InvalidKeyLength {
            minimum: DES_BLOCK_SIZE,
            actual: key.len(),
        });
    }
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let cipher = CbcEncryptor::<Des>::new_from_slices(&key.as_bytes()[..DES_BLOCK_SIZE], &DES_IV)
        .map_err(|_| DesError::InvalidKeyLength {
            minimum: DES_BLOCK_SIZE,
            actual: key.len(),
        })?;
    Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(data))
}

/// Compatibility wrapper. Use [`try_encrypt_bytes`] when failures need to be
/// distinguished from empty input.
pub fn encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    try_encrypt_bytes(data, key).unwrap_or_default()
}

/// DES/CBC/PKCS#7 decryption. The protocol uses the first eight key bytes.
pub fn decrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, DesError> {
    if key.len() < DES_BLOCK_SIZE {
        return Err(DesError::InvalidKeyLength {
            minimum: DES_BLOCK_SIZE,
            actual: key.len(),
        });
    }
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if data.len() % DES_BLOCK_SIZE != 0 {
        return Err(DesError::InvalidBlockLength);
    }
    let cipher = CbcDecryptor::<Des>::new_from_slices(&key.as_bytes()[..DES_BLOCK_SIZE], &DES_IV)
        .map_err(|_| DesError::InvalidKeyLength {
            minimum: DES_BLOCK_SIZE,
            actual: key.len(),
        })?;
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|_| DesError::InvalidPadding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbc_round_trip_uses_protocol_iv() {
        let ciphertext = try_encrypt_bytes(b"test", "abcdefgh").unwrap();
        assert_eq!(decrypt_bytes(&ciphertext, "abcdefgh").unwrap(), b"test".to_vec());
    }

    #[test]
    fn rejects_short_keys_and_misaligned_ciphertext() {
        assert!(matches!(
            try_encrypt_bytes(b"data", "short"),
            Err(DesError::InvalidKeyLength { .. })
        ));
        assert_eq!(
            decrypt_bytes(&[0u8; 3], "abcdefgh"),
            Err(DesError::InvalidBlockLength)
        );
    }
}
