use base64::{
    engine::general_purpose,
    Engine as _,
};

use md5;
use regex::Regex;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::encrypt_util::ENCRYPTION_KEY;

use crate::aes; 

pub struct SignUtil;
const EMAIL_PATTERN: &'static str = r"^(([\w-]+\.)+[\w-]+|([a-zA-Z]{1}|[\w-]{2,}))@((([0-1]?[0-9]{1,2}|25[0-5]|2[0-4][0-9])\.([0-1]?[0-9]{1,2}|25[0-5]|2[0-4][0-9])\.([0-1]?[0-9]{1,2}|25[0-5]|2[0-4][0-9])\.([0-1]?[0-9]{1,2}|25[0-5]|2[0-4][0-9])){1}|([a-zA-Z]+[\w-]+\.)+[a-zA-Z]{2,4})$";

impl SignUtil {
    
    
    pub fn calculate_md5_hash(input: &str) -> String {
        format!("{:x}", md5::compute(input.as_bytes()))
    }

    pub fn encrypt(plain_text: &str) -> String {
        let enc = aes::encrypt_cbc(plain_text.as_bytes(), ENCRYPTION_KEY);
        general_purpose::STANDARD.encode(enc)
    }

    pub fn decrypt(encrypted_text: &str) -> String {
        let raw = general_purpose::STANDARD
            .decode(encrypted_text)
            .expect("Invalid base64");
        let dec = aes::decrypt_cbc(&raw, ENCRYPTION_KEY)
            .expect("AES decryption failed");
        String::from_utf8(dec).expect("Invalid UTF-8")
    }
    
    pub fn get_timestamp() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        secs.to_string()
    }
    
    pub fn validate_email(email: &str) -> bool {
        if email.is_empty() {
            return false;
        }
        Regex::new(EMAIL_PATTERN)
            .map(|re| re.is_match(email))
            .unwrap_or(false)
    }
}
