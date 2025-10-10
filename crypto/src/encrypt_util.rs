use crate::aes;
use crate::des;
use crate::xxtea;
use crate::rc4::{Rc4Creator, Rc4Transform};
use base64::{engine::general_purpose, Engine as _};

/// Constants (mirroring C#)
pub const DES_IV: [u8; 8] = [111, 151, 50, 205, 123, 222, 185, 45];
pub const AES_KEY: &str = "1234567890123456";
pub const AES_IV: &[u8; 16] = b"abcdefghijklmnop";
pub const XXTEA_KEY: &str = "1234567890123456";
pub const ENCRYPTION_KEY: &str = "HaoYouFIgFIFkgfg";

pub struct EncryptUtil;

impl EncryptUtil {
    // ---- SplitBytes ----
    pub fn split_bytes(data: &[u8], once_split_size: usize) -> Vec<Vec<u8>> {
        if data.is_empty() {
            return vec![];
        }
        let mut result = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let end = usize::min(offset + once_split_size, data.len());
            result.push(data[offset..end].to_vec());
            offset = end;
        }
        result
    }

    // ---- DES ----
    pub fn des_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        des::encrypt_bytes(data, key)
    }

    pub fn des_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        des::decrypt_bytes(data, key).unwrap_or_else(|_| {
            vec![]
        })
    }

    pub fn des_encrypt_string(s: &str, key: &str) -> String {
        let enc = Self::des_encrypt_bytes(s.as_bytes(), key);
        general_purpose::STANDARD.encode(enc)
    }

    pub fn des_decrypt_string(s: &str, key: &str) -> String {
        let raw = general_purpose::STANDARD.decode(s).unwrap();
        let dec = Self::des_decrypt_bytes(&raw, key);
        String::from_utf8(dec).unwrap()
    }

    // ---- AES ----
    pub fn aes_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        aes::encrypt_bytes(data, key)
    }

    pub fn aes_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        aes::decrypt_bytes(data, key)
    }

    pub fn aes_encrypt_string(s: &str, key: &str) -> String {
        let enc = Self::aes_encrypt_bytes(s.as_bytes(), key);
        general_purpose::STANDARD.encode(enc)
    }

    pub fn aes_decrypt_string(s: &str, key: &str) -> String {
        let raw = general_purpose::STANDARD.decode(s).unwrap();
        let dec = Self::aes_decrypt_bytes(&raw, key);
        String::from_utf8(dec).unwrap()
    }

    pub fn aes_encrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
        aes::encrypt_no_padding(data, key)
    }

    pub fn aes_decrypt_no_padding(data: &[u8], key: &str) -> Vec<u8> {
        aes::decrypt_no_padding(data, key).unwrap()
    }

    // ---- RC4 ----
    pub fn rc4_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        if data.is_empty() {
            return vec![];
        }
        let creator = Rc4Creator::new();
        let mut transform = creator.create_encryptor(key.as_bytes());
        transform.transform_final_block(data)
    }

    pub fn rc4_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        if data.is_empty() {
            return vec![];
        }
        let creator = Rc4Creator::new();
        let mut transform = creator.create_decryptor(key.as_bytes());
        transform.transform_final_block(data)
    }

    // ---- XXTEA ----
    pub fn xxtea_encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        xxtea::encrypt_bytes(data, key)
    }

    pub fn xxtea_decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
        xxtea::decrypt_bytes(data, key)
    }
}
