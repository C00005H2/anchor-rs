use aes::Aes128;
use cipher::{
    BlockEncryptMut, BlockDecryptMut, KeyInit, KeyIvInit,
    block_padding::{Pkcs7, NoPadding, UnpadError},
};
use ecb::{Encryptor as EcbEncryptor, Decryptor as EcbDecryptor};
use cbc::{Encryptor as CbcEncryptor, Decryptor as CbcDecryptor};

/// AES-128 ECB
type AesEcbEnc = EcbEncryptor<Aes128>;
type AesEcbDec = EcbDecryptor<Aes128>;

/// AES-128 ECB (NoPadding)
type AesEcbNoPadEnc = EcbEncryptor<Aes128>;
type AesEcbNoPadDec = EcbDecryptor<Aes128>;

/// AES-128 CBC
type AesCbcEnc = CbcEncryptor<Aes128>;
type AesCbcDec = CbcDecryptor<Aes128>;

#[derive(Debug)]
pub enum AesError {
    InvalidLength,
    PaddingError,
    Utf8Error,
}

impl std::fmt::Display for AesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for AesError {}

/// AES/ECB/PKCS7 encrypt
pub fn encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    let cipher = AesEcbEnc::new_from_slice(key.as_bytes()).unwrap();
    cipher.encrypt_padded_vec_mut::<Pkcs7>(data)
}

/// AES/ECB/PKCS7 decrypt
pub fn decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    let cipher = AesEcbDec::new_from_slice(key.as_bytes()).unwrap();
    cipher.decrypt_padded_vec_mut::<Pkcs7>(data).unwrap()
}

/// AES/ECB/NoPadding encrypt
pub fn encrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
    let cipher = AesEcbNoPadEnc::new_from_slice(key.as_bytes()).unwrap();
    cipher.encrypt_padded_vec_mut::<NoPadding>(data)
}

/// AES/ECB/NoPadding decrypt
pub fn decrypt_no_padding(data: &[u8], key: &str) -> Result<Vec<u8>, UnpadError> {
    let cipher = AesEcbNoPadDec::new_from_slice(key.as_bytes()).unwrap();
    cipher.decrypt_padded_vec_mut::<NoPadding>(data)
}

/// AES/CBC/PKCS7 encrypt (IV = 0)
pub fn encrypt_cbc(data: &[u8], key: &str) -> Vec<u8> {
    let iv = [0u8; 16];
    let cipher = AesCbcEnc::new_from_slices(key.as_bytes(), &iv).unwrap();
    cipher.encrypt_padded_vec_mut::<Pkcs7>(data)
}

/// AES/CBC/PKCS7 decrypt (IV = 0)

pub fn decrypt_cbc(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
    if key.len() != 16 {
        return Err(AesError::InvalidLength);
    }

    let iv = [0u8; 16];
    let cipher = AesCbcDec::new_from_slices(key.as_bytes(), &iv)
        .map_err(|_| AesError::InvalidLength)?;

    let decrypted = cipher.decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|_| AesError::PaddingError)?;

    Ok(decrypted)
}
