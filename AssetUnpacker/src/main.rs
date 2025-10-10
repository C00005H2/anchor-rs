mod decrypt_lua;

use std::env;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use decrypt_lua::decrypt_lua;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: asset_unpacker <encrypted_dir> --lua");
        return;
    }

    let input_dir = &args[1];
    let mode = &args[2];

    let output_dir = Path::new(input_dir)
        .parent()
        .unwrap_or_else(|| Path::new(input_dir))
        .join("output2");

    fs::create_dir_all(&output_dir).unwrap();

    for entry in WalkDir::new(input_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let file = entry.path();
        let relative = file.strip_prefix(input_dir).unwrap();
        let output_path = output_dir.join(relative);

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        let encrypted_bytes = match fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Failed to read {}: {}", file.display(), e);
                continue;
            }
        };

        let maybe_decrypted = match mode.as_str() {
            "--lua" => decrypt_lua(&encrypted_bytes),
            _ => {
                eprintln!("Unknown mode: {}", mode);
                None
            }
        };

        match maybe_decrypted {
            Some(decrypted) => {
                // Always write output, even if no Lua header
                if let Err(e) = fs::write(&output_path, decrypted) {
                    eprintln!("Failed to write {}: {}", output_path.display(), e);
                } else {
                    println!("Decrypted: {} -> {}", file.display(), output_path.display());
                }
            }
            None => {
                println!("Failed to decrypt: {}", file.display());
            }
        }

    }
}
