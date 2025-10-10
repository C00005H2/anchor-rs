use des::Des;
use cipher::{
    BlockEncryptMut, BlockDecryptMut, KeyIvInit,
    block_padding::{Pkcs7, UnpadError},
};
use cbc::{Encryptor as CbcEncryptor, Decryptor as CbcDecryptor};

type DesCbcEnc = CbcEncryptor<Des>;
type DesCbcDec = CbcDecryptor<Des>;

/// Fixed DES IV (matches C# mDesMKeys)
pub const DES_IV: [u8; 8] = [111, 151, 50, 205, 123, 222, 185, 45];

/// DES/CBC/PKCS7 encrypt
pub fn encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    let key_bytes = &key.as_bytes()[..8];
    let cipher = DesCbcEnc::new_from_slices(key_bytes, &DES_IV).unwrap();
    cipher.encrypt_padded_vec_mut::<Pkcs7>(data)
}

/// DES/CBC/PKCS7 decrypt
pub fn decrypt_bytes(data: &[u8], key: &str) -> Result<Vec<u8>, UnpadError> {
    if data.is_empty() {
        return Ok(vec![]);
    }
    let key_bytes = &key.as_bytes()[..8];
    let cipher = DesCbcDec::new_from_slices(key_bytes, &DES_IV).unwrap();
    cipher.decrypt_padded_vec_mut::<Pkcs7>(&*data.to_vec())
}
