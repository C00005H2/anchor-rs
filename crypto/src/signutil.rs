use base64::{engine::general_purpose, Engine as _};
use md5;
use regex::Regex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::aes;
use crate::encrypt_util::ENCRYPTION_KEY;

pub struct SignUtil;

const EMAIL_PATTERN: &str = r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$";

impl SignUtil {
    pub fn calculate_md5_hash(input: &str) -> String {
        format!("{:x}", md5::compute(input.as_bytes()))
    }

    pub fn encrypt(plain_text: &str) -> String {
        let enc = aes::encrypt_cbc(plain_text.as_bytes(), ENCRYPTION_KEY);
        general_purpose::STANDARD.encode(enc)
    }

    /// Decrypt a Base64-encoded AES/CBC value without panicking on malformed
    /// input. The compatibility [`decrypt`](Self::decrypt) method returns an
    /// empty string on error.
    pub fn try_decrypt(
        encrypted_text: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let raw = general_purpose::STANDARD.decode(encrypted_text)?;
        let decrypted = aes::decrypt_cbc(&raw, ENCRYPTION_KEY)?;
        Ok(String::from_utf8(decrypted)?)
    }

    pub fn decrypt(encrypted_text: &str) -> String {
        Self::try_decrypt(encrypted_text).unwrap_or_default()
    }

    pub fn get_timestamp() -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string()
    }

    pub fn validate_email(email: &str) -> bool {
        if email.is_empty() {
            return false;
        }
        static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
        EMAIL_RE
            .get_or_init(|| Regex::new(EMAIL_PATTERN).expect("EMAIL_PATTERN is a valid regex"))
            .is_match(email)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_handles_invalid_base64_without_panicking() {
        assert!(SignUtil::try_decrypt("bad base64").is_err());
        assert_eq!(SignUtil::decrypt("bad base64"), "");
    }

    #[test]
    fn email_validation_reuses_compiled_pattern() {
        assert!(SignUtil::validate_email("user@example.com"));
        assert!(!SignUtil::validate_email("not an email"));
        assert!(!SignUtil::validate_email(""));
    }
}
