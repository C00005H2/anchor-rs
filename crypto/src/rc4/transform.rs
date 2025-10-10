
pub struct Rc4Transform {
    key: Vec<u8>,
    key_len: usize,
    s: [u8; 256],
    i: u8,
    j: u8,
    disposed: bool,
}

impl Rc4Transform {
    pub fn new(key: &[u8]) -> Self {
        let mut rc4 = Rc4Transform {
            key: key.to_vec(),
            key_len: key.len(),
            s: [0u8; 256],
            i: 0,
            j: 0,
            disposed: false,
        };
        rc4.init();
        rc4
    }

    fn init(&mut self) {
        for i in 0..256 {
            self.s[i] = i as u8;
        }
        self.i = 0;
        self.j = 0;
        let mut j: usize = 0;
        for i in 0..256 {
            j = (j + self.s[i] as usize + self.key[i % self.key_len] as usize) % 256;
            self.s.swap(i, j);
        }
    }

    pub fn can_reuse_transform(&self) -> bool { true }
    pub fn can_transform_multiple_blocks(&self) -> bool { true }
    pub fn input_block_size(&self) -> usize { 1 }
    pub fn output_block_size(&self) -> usize { 1 }

    pub fn transform_block(&mut self, input: &[u8]) -> Vec<u8> {
        if self.disposed {
            panic!("ObjectDisposedException: Rc4Transform");
        }
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let idx = (self.s[self.i as usize] as usize + self.s[self.j as usize] as usize) % 256;
            out.push(b ^ self.s[idx]);
        }
        out
    }

    pub fn transform_final_block(&mut self, input: &[u8]) -> Vec<u8> {
        if self.disposed {
            panic!("ObjectDisposedException: Rc4Transform");
        }
        let out = self.transform_block(input);
        self.init(); 
        out
    }

    pub fn dispose(&mut self) {
        self.key.fill(0);
        self.s.fill(0);
        self.i = 0;
        self.j = 0;
        self.disposed = true;
    }
}
