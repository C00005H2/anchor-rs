use crypto::{asset_setting::AssetSetting, encrypt_util::EncryptUtil};

/// Attempt to decrypt Lua bytes with DES using AssetSetting::CommonKey.
pub fn decrypt_lua(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() {
        return Some(bytes.to_vec());
    }

    let key = AssetSetting::common_key();

    // Catch unwinding panics from bad decryption attempts.
    match std::panic::catch_unwind(|| EncryptUtil::des_decrypt_bytes(bytes, key)) {
        Ok(data) => Some(data),
        Err(_) => None,
    }
}

