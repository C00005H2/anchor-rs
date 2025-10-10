/// Small preview helper for logs
pub fn hex_preview(data: &[u8], max: usize) -> String {
    data.iter()
        .take(max)
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Full hex dump with ASCII representation
pub fn full_hexdump(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{:08X}  ", i * 16);
        for b in chunk {
            print!("{:02X} ", b);
        }
        for _ in chunk.len()..16 {
            print!("   ");
        }
        print!(" ");
        for b in chunk {
            let c = if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!();
    }
}