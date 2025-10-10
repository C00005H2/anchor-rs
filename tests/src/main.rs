use crypto::{EncryptUtil, SignUtil, XXTEA_KEY};

fn main() {
    let msg = "hello world";

    // AES roundtrip
    let enc = EncryptUtil::aes_encrypt_string(msg, "1234567890123456");
    let dec = EncryptUtil::aes_decrypt_string(&enc, "1234567890123456");
    println!("AES: {msg} -> {enc} -> {dec}");

    // DES roundtrip
    let enc = EncryptUtil::des_encrypt_string("test", "abcdefgh");
    let dec = EncryptUtil::des_decrypt_string(&enc, "abcdefgh");
    println!("DES: test -> {enc} -> {dec}");

    // XXTEA
    let enc = EncryptUtil::xxtea_encrypt_bytes(msg.as_bytes(), XXTEA_KEY);
    let dec = EncryptUtil::xxtea_decrypt_bytes(&enc, XXTEA_KEY);
    println!("XXTEA: {:?}", String::from_utf8(dec).unwrap());

    let enc = SignUtil::encrypt("JzqCQVRoN98nY9WK0pQUU0gvT08rOUh1MmtYdlk4YmZrUTJSS0ZxU09yc3JoNlZNS2tvVVJqajRWcVRHM1pENEJrTUpQSEk1SHRaNmJUdkxQK3ZhWlJaRDhVa3ZONmVkenU1d0ZUbVdZaEN6NWhramMvUnhrbG5rMXdLOTdqd0NnN3M5MlplbWF6d0RIZzFSS2NyeUdRMHBiV3ZGUW5yQnZxZVBiS0xwYmd0anZBS0ZwVFFYcy9TNnFTcU5nQW1OcXdXeWZpR2lXY045R0pvRw%3D%3D");
    let dec = SignUtil::decrypt(&enc);
    println!("DES: {:?}", dec);
}
