use md5;
use std::fmt::Write;

pub struct MD5Util;

impl MD5Util {
    pub fn get_md5_by_string(content: &str) -> String {
        let digest = md5::compute(content.as_bytes());
        let mut out = String::new();
        for byte in digest.0 {
            write!(&mut out, "{:02x}", byte).unwrap();
        }
        out
    }
}
