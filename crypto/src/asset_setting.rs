use crate::md5util::MD5Util;
use std::sync::OnceLock;

/// AssetSetting equivalent from C#
pub struct AssetSetting;

impl AssetSetting {
    /// Lazy-initialized CommonKey ("qdiazawh" → md5 → last 8 chars)
    pub fn common_key() -> &'static str {
        static COMMON_KEY: OnceLock<String> = OnceLock::new();
        COMMON_KEY.get_or_init(|| {
            let base = "qdiazawh";
            let md5 = MD5Util::get_md5_by_string(base);
            let len = md5.len();
            md5[len - 8..].to_string()
        })
    }

    /// Lazy-initialized ProtocolKey ("sumvhelz" → md5 → last 8 chars)
    pub fn protocol_key() -> &'static str {
        static PROTOCOL_KEY: OnceLock<String> = OnceLock::new();
        PROTOCOL_KEY.get_or_init(|| {
            let base = "sumvhelz";
            let md5 = MD5Util::get_md5_by_string(base);
            let len = md5.len();
            md5[len - 8..].to_string()
        })
    }
}
