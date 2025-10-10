/// RC4 algorithm "container" (mimics .NET SymmetricAlgorithm style).
pub struct Rc4 {
    pub key_size: usize,
    pub block_size: usize,
    pub feedback_size: usize,
    pub legal_block_sizes: Vec<(usize, usize, usize)>, // (min, max, step)
    pub legal_key_sizes: Vec<(usize, usize, usize)>,
}

impl Rc4 {
    pub fn new() -> Self {
        Rc4 {
            key_size: 128,
            block_size: 8,
            feedback_size: 0,
            legal_block_sizes: vec![(8, 8, 0)],
            legal_key_sizes: vec![(8, 2048, 8)],
        }
    }

    pub fn generate_iv(&self) -> Vec<u8> {
        vec![0] // RC4 has no IV, keep for API compatibility
    }

    pub fn generate_key(&self, size: usize) -> Vec<u8> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..size).map(|_| rng.gen()).collect()
    }
}
