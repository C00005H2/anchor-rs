use super::Rc4Transform;

pub struct Rc4Creator {
    disposed: bool,
}

impl Rc4Creator {
    pub fn new() -> Self {
        Rc4Creator { disposed: false }
    }

    pub fn create_encryptor(&self, key: &[u8]) -> Rc4Transform {
        self.create_decryptor(key) // same for RC4
    }

    pub fn create_decryptor(&self, key: &[u8]) -> Rc4Transform {
        if self.disposed {
            panic!("ObjectDisposedException: Rc4Creator");
        }
        if key.is_empty() || key.len() > 256 {
            panic!("CryptographicException: Invalid key length");
        }
        Rc4Transform::new(key)
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }
}
