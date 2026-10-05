use crate::aes::{self, AesError};
use crate::des::{self, DesError};
use crate::rc4::{Rc4Creator, Rc4Transform};
use crate::xxtea;
use base64::{engine::general_purpose, Engine as _};
use std::error::Error;

pub const DES_IV: [u8; 8] = [111, 151, 50, 205, 123, 222, 185, 45];
pub const AES_KEY: &str = "1234567890123456";
pub const AES_IV: &[u8; 16] = b"abcdefghijklmnop";
pub const XXTEA_KEY: &str = "1234567890123456";
pub const ENCRYPTION_KEY: &str = "HaoYouFIgFIFkgfg";

pub struct EncryptUtil;

impl EncryptUtil {
    /// Split bytes into chunks. A zero chunk size is rejected rather than
    /// looping forever; use [`try_split_bytes`] if that distinction matters.
    pub fn split_bytes(data: &[u8], once_split_size: usize) -> Vec<Vec<u8>> {
        Self::try_split_bytes(data, once_split_size).unwrap_or_default()
    }

    pub fn try_split_bytes(data: &[u8], once_split_size: usize) -> Option<Vec<Vec<u8>>> {
        if once_split_size == 0 {
            return None;
        }
        Some(data.chunks(once_split_size).map(|chunk| chunk.to_vec()).collect())
    }

    pub fn try_des_encrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, DesError> {
        des::try_encrypt_bytes(data, key)
    }

    pub fn des_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_des_encrypt_bytes(data, key).unwrap_or_default()
    }

    pub fn try_des_decrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, DesError> {
        des::decrypt_bytes(data, key)
    }

    /// Compatibility wrapper. Prefer [`try_des_decrypt_bytes`] to tell an
    /// invalid ciphertext from an empty plaintext.
    pub fn des_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_des_decrypt_bytes(data, key).unwrap_or_default()
    }

    pub fn des_encrypt_string(s: &str, key: &str) -> String {
        let enc = Self::des_encrypt_bytes(s.as_bytes(), key);
        general_purpose::STANDARD.encode(enc)
    }

    pub fn try_des_decrypt_string(
        s: &str,
        key: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let raw = general_purpose::STANDARD.decode(s)?;
        let dec = Self::try_des_decrypt_bytes(&raw, key)?;
        Ok(String::from_utf8(dec)?)
    }

    /// Compatibility wrapper. Invalid Base64, ciphertext, or UTF-8 yields an
    /// empty string; use [`try_des_decrypt_string`] for an error.
    pub fn des_decrypt_string(s: &str, key: &str) -> String {
        Self::try_des_decrypt_string(s, key).unwrap_or_default()
    }

    pub fn try_aes_encrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
        aes::try_encrypt_bytes(data, key)
    }

    pub fn aes_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_aes_encrypt_bytes(data, key).unwrap_or_default()
    }

    pub fn try_aes_decrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
        aes::try_decrypt_bytes(data, key)
    }

    /// Compatibility wrapper. Prefer [`try_aes_decrypt_bytes`] to distinguish
    /// invalid ciphertext from empty plaintext.
    pub fn aes_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_aes_decrypt_bytes(data, key).unwrap_or_default()
    }

    pub fn aes_encrypt_string(s: &str, key: &str) -> String {
        let enc = Self::aes_encrypt_bytes(s.as_bytes(), key);
        general_purpose::STANDARD.encode(enc)
    }

    pub fn try_aes_decrypt_string(
        s: &str,
        key: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let raw = general_purpose::STANDARD.decode(s)?;
        let dec = Self::try_aes_decrypt_bytes(&raw, key)?;
        Ok(String::from_utf8(dec)?)
    }

    /// Compatibility wrapper. Invalid Base64, ciphertext, or UTF-8 yields an
    /// empty string; use [`try_aes_decrypt_string`] for an error.
    pub fn aes_decrypt_string(s: &str, key: &str) -> String {
        Self::try_aes_decrypt_string(s, key).unwrap_or_default()
    }

    pub fn try_aes_encrypt_no_padding(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
        aes::try_encrypt_no_padding(data, key)
    }

    pub fn aes_encrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_aes_encrypt_no_padding(data, key).unwrap_or_default()
    }

    pub fn try_aes_decrypt_no_padding(data: &[u8], key: &str) -> Result<Vec<u8>, AesError> {
        aes::decrypt_no_padding(data, key)
    }

    pub fn aes_decrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
        Self::try_aes_decrypt_no_padding(data, key).unwrap_or_default()
    }

    pub fn rc4_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        if data.is_empty() || key.is_empty() || key.len() > 256 {
            return Vec::new();
        }
        let creator = Rc4Creator::new();
        let mut transform: Rc4Transform = creator.create_encryptor(key.as_bytes());
        transform.transform_final_block(data)
    }

    pub fn rc4_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        if data.is_empty() || key.is_empty() || key.len() > 256 {
            return Vec::new();
        }
        let creator = Rc4Creator::new();
        let mut transform = creator.create_decryptor(key.as_bytes());
        transform.transform_final_block(data)
    }

    pub fn xxtea_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        xxtea::encrypt_bytes(data, key)
    }

    pub fn xxtea_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        xxtea::decrypt_bytes(data, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_handles_empty_data_and_rejects_zero_size() {
        assert_eq!(EncryptUtil::try_split_bytes(&[], 4), Some(vec![]));
        assert_eq!(
            EncryptUtil::try_split_bytes(b"abcdef", 2),
            Some(vec![b"ab".to_vec(), b"cd".to_vec(), b"ef".to_vec()])
        );
        assert_eq!(EncryptUtil::try_split_bytes(b"data", 0), None);
        assert!(EncryptUtil::split_bytes(b"data", 0).is_empty());
    }

    #[test]
    fn string_decryption_reports_bad_base64_instead_of_panicking() {
        assert!(EncryptUtil::try_aes_decrypt_string("not base64", AES_KEY).is_err());
        assert!(EncryptUtil::try_des_decrypt_string("not base64", "abcdefgh").is_err());
    }
}
