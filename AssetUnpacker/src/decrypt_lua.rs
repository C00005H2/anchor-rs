use crypto::{asset_setting::AssetSetting, EncryptUtil};

/// Decrypt Lua asset bytes with the game's DES key.
pub fn decrypt_lua(bytes: &[u8]) -> Result<Vec<u8>, crypto::DesError> {
    EncryptUtil::try_des_decrypt_bytes(bytes, AssetSetting::common_key())
}
