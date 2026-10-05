use aes::Aes128;
use cbc::{Decryptor as CbcDecryptor, Encryptor as CbcEncryptor};
use cipher::{
    block_padding::{NoPadding, Pkcs7},
    BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit,
};
use ecb::{Decryptor as EcbDecryptor, Encryptor as EcbEncryptor};

const AES_BLOCK_SIZE: usize = 16;

/// Errors returned by the checked AES helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesError {
    InvalidKeyLength { expected: usize, actual: usize },
    InvalidBlockLength,
    InvalidPadding,
}

impl std::fmt::Display for AesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKeyLength { expected, actual } => {
                write!(f, "invalid AES key length: expected {expected}, got {actual}")
            }
            Self::InvalidBlockLength => write!(f, "AES input length is not a multiple of 16 bytes"),
            Self::InvalidPadding => write!(f, "invalid AES padding"),
        }
    }
}

impl std::error::Error for AesError {}

/// AES-128 ECB with PKCS#7 padding. Empty input remains an empty byte vector
/// for compatibility with the protocol helpers in this crate.
pub fn try_encrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let cipher = EcbEncryptor::<Aes128>::new_from_slice(key.as_bytes()).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(data))
}

/// Compatibility wrapper. Use [`try_encrypt_bytes`] when invalid input must
/// be distinguished from an empty result.
pub fn encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    try_encrypt_bytes(data, key).unwrap_or_default()
}

/// AES-128 ECB with PKCS#7 padding.
pub fn try_decrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let cipher = EcbDecryptor::<Aes128>::new_from_slice(key.as_bytes()).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(AesError::InvalidBlockLength);
    }
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|_| AesError::InvalidPadding)
}

/// Compatibility wrapper. Use [`try_decrypt_bytes`] when invalid input must
/// be distinguished from an empty result.
pub fn decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    try_decrypt_bytes(data, key).unwrap_or_default()
}

/// AES-128 ECB with no padding. Input must already be block-aligned.
pub fn try_encrypt_no_padding(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(AesError::InvalidBlockLength);
    }
    let cipher = EcbEncryptor::<Aes128>::new_from_slice(key.as_bytes()).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    Ok(cipher.encrypt_padded_vec_mut::<NoPadding>(data))
}

/// Compatibility wrapper. Use [`try_encrypt_no_padding`] to inspect errors.
pub fn encrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
    try_encrypt_no_padding(data, key).unwrap_or_default()
}

/// AES-128 ECB with no padding. Input must already be block-aligned.
pub fn decrypt_no_padding(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(AesError::InvalidBlockLength);
    }
    let cipher = EcbDecryptor::<Aes128>::new_from_slice(key.as_bytes()).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    cipher
        .decrypt_padded_vec_mut::<NoPadding>(data)
        .map_err(|_| AesError::InvalidPadding)
}

/// AES-128 CBC with a zero IV and PKCS#7 padding.
pub fn try_encrypt_cbc(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let iv = [0u8; AES_BLOCK_SIZE];
    let cipher = CbcEncryptor::<Aes128>::new_from_slices(key.as_bytes(), &iv).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(data))
}

/// Compatibility wrapper. Use [`try_encrypt_cbc`] to inspect errors.
pub fn encrypt_cbc(data: &[u8], key: &str) -> Vec<u8> {
    try_encrypt_cbc(data, key).unwrap_or_default()
}

/// AES-128 CBC with a zero IV and PKCS#7 padding.
pub fn decrypt_cbc(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let iv = [0u8; AES_BLOCK_SIZE];
    let cipher = CbcDecryptor::<Aes128>::new_from_slices(key.as_bytes(), &iv).map_err(|_| {
        AesError::InvalidKeyLength {
            expected: AES_BLOCK_SIZE,
            actual: key.len(),
        }
    })?;
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(AesError::InvalidBlockLength);
    }
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|_| AesError::InvalidPadding)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "1234567890123456";

    #[test]
    fn ecb_pkcs7_round_trips_various_lengths() {
        let plaintexts: [&[u8]; 2] = [b"hello", b"sixteen-byte-msg"];
        for plaintext in plaintexts {
            let ciphertext = try_encrypt_bytes(plaintext, KEY).unwrap();
            assert_eq!(try_decrypt_bytes(&ciphertext, KEY).unwrap(), plaintext.to_vec());
        }
    }

    #[test]
    fn rejects_bad_keys_and_malformed_ciphertext_without_panicking() {
        assert!(matches!(
            try_encrypt_bytes(b"data", "short"),
            Err(AesError::InvalidKeyLength { .. })
        ));
        assert_eq!(try_decrypt_bytes(&[0u8; 3], KEY), Err(AesError::InvalidBlockLength));
        assert!(decrypt_bytes(&[0u8; 3], KEY).is_empty());
    }

    #[test]
    fn no_padding_requires_complete_blocks() {
        assert_eq!(
            try_encrypt_no_padding(b"short", KEY),
            Err(AesError::InvalidBlockLength)
        );
        let ciphertext = try_encrypt_no_padding(b"sixteen-byte-msg", KEY).unwrap();
        assert_eq!(
            try_decrypt_no_padding(&ciphertext, KEY).unwrap(),
            b"sixteen-byte-msg".to_vec()
        );
    }

    #[test]
    fn cbc_round_trip() {
        let ciphertext = try_encrypt_cbc(b"cbc data", KEY).unwrap();
        assert_eq!(decrypt_cbc(&ciphertext, KEY).unwrap(), b"cbc data".to_vec());
    }
}
