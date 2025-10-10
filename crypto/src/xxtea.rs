
pub fn encrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    let v = to_u32_array(data, true);
    let k = to_u32_array(key.as_bytes(), false);
    to_byte_array(xxtea_encrypt(v, k), false)
}

pub fn decrypt_bytes(data: &[u8], key: &str) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    let v = to_u32_array(data, false);
    let k = to_u32_array(key.as_bytes(), false);
    to_byte_array(xxtea_decrypt(v, k), true)
}

fn xxtea_encrypt(mut v: Vec<u32>, mut k: Vec<u32>) -> Vec<u32> {
    let num: usize = v.len() - 1;
    if num < 1 {
        return v;
    }
    if k.len() < 4 {
        k.resize(4, 0);
    }

    let mut num2: u32 = v[num];
    let mut num3: u32 = v[0];
    let num4: u32 = 0x9E3779B9; // 2654435769
    let mut num5: u32 = 0;
    let mut num6: i32 = (6 + 52 / (num + 1)) as i32;

    while num6 > 0 {
        num6 -= 1;
        num5 = num5.wrapping_add(num4);
        let num7: usize = ((num5 >> 2) & 3) as usize;

        let mut i: usize = 0;
        while i < num {
            num3 = v[i + 1];
            v[i] = v[i].wrapping_add(
                ((num2 >> 5) ^ (num3 << 2))
                    .wrapping_add((num3 >> 3) ^ (num2 << 4))
                    ^ ((num5 ^ num3).wrapping_add(k[(i & 3) ^ num7] ^ num2)),
            );
            num2 = v[i];
            i += 1;
        }

        num3 = v[0];
        v[num] = v[num].wrapping_add(
            ((num2 >> 5) ^ (num3 << 2))
                .wrapping_add((num3 >> 3) ^ (num2 << 4))
                ^ ((num5 ^ num3).wrapping_add(k[(i & 3) ^ num7] ^ num2)),
        );
        num2 = v[num];
    }

    v
}

fn xxtea_decrypt(mut v: Vec<u32>, mut k: Vec<u32>) -> Vec<u32> {
    let num: usize = v.len() - 1;
    if num < 1 {
        return v;
    }
    if k.len() < 4 {
        k.resize(4, 0);
    }

    let mut num2: u32 = v[num];
    let mut num3: u32 = v[0];
    let num4: u32 = 0x9E3779B9;

    let mut num5: u32 = ((6 + 52 / (num + 1)) as u32).wrapping_mul(num4);

    while num5 != 0 {
        let num6: usize = ((num5 >> 2) & 3) as usize;
        let mut num7: usize = num;

        while num7 > 0 {
            num2 = v[num7 - 1];
            v[num7] = v[num7].wrapping_sub(
                ((num2 >> 5) ^ (num3 << 2))
                    .wrapping_add((num3 >> 3) ^ (num2 << 4))
                    ^ ((num5 ^ num3).wrapping_add(k[(num7 & 3) ^ num6] ^ num2)),
            );
            num3 = v[num7];
            num7 -= 1;
        }

        num2 = v[num];
        v[0] = v[0].wrapping_sub(
            ((num2 >> 5) ^ (num3 << 2))
                .wrapping_add((num3 >> 3) ^ (num2 << 4))
                ^ ((num5 ^ num3).wrapping_add(k[(num7 & 3) ^ num6] ^ num2)),
        );
        num3 = v[0];

        num5 = num5.wrapping_sub(num4);
    }

    v
}

fn to_u32_array(data: &[u8], include_len: bool) -> Vec<u32> {
    let mut n = if data.len() % 4 == 0 { data.len() / 4 } else { data.len() / 4 + 1 };
    let mut result: Vec<u32>;
    if include_len {
        result = vec![0; n + 1];
        result[n] = data.len() as u32;
    } else {
        result = vec![0; n];
    }
    for i in 0..data.len() {
        result[i / 4] |= (data[i] as u32) << ((i % 4) * 8);
    }
    result
}

fn to_byte_array(data: Vec<u32>, include_len: bool) -> Vec<u8> {
    let mut n = data.len() * 4;
    if include_len {
        let m = data[data.len() - 1] as usize;
        if m < n - 3 || m > n { return vec![]; }
        n = m;
    }
    let mut result = vec![0u8; n];
    for i in 0..n {
        result[i] = (data[i / 4] >> ((i % 4) * 8)) as u8;
    }
    result
}

