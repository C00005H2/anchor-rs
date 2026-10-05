mod decrypt_lua;

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use walkdir::WalkDir;

use decrypt_lua::decrypt_lua;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("asset_unpacker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: asset_unpacker <encrypted_dir> --lua [output_dir]".into());
    }

    let mode = args[1]
        .to_str()
        .ok_or("mode must be valid UTF-8")?;
    if mode != "--lua" {
        return Err(format!("unsupported mode '{mode}'; expected --lua").into());
    }

    let input_dir = fs::canonicalize(PathBuf::from(&args[0]))?;
    if !input_dir.is_dir() {
        return Err(format!("input path is not a directory: {}", input_dir.display()).into());
    }

    let output_dir = if let Some(path) = args.get(2) {
        PathBuf::from(path)
    } else {
        input_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("output2")
    };
    fs::create_dir_all(&output_dir)?;
    let output_dir = fs::canonicalize(output_dir)?;
    if output_dir.starts_with(&input_dir) {
        return Err(format!(
            "output directory must not be inside the input directory (input: {}, output: {})",
            input_dir.display(),
            output_dir.display()
        )
        .into());
    }

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for result in WalkDir::new(&input_dir).follow_links(false) {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("asset_unpacker: failed to walk input: {error}");
                failed += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let source = entry.path();
        let relative = match source.strip_prefix(&input_dir) {
            Ok(relative) => relative,
            Err(error) => {
                eprintln!("asset_unpacker: cannot make {} relative: {error}", source.display());
                failed += 1;
                continue;
            }
        };
        let destination = output_dir.join(relative);

        if let Some(parent) = destination.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("asset_unpacker: failed to create {}: {error}", parent.display());
                failed += 1;
                continue;
            }
        }

        let encrypted_bytes = match fs::read(source) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("asset_unpacker: failed to read {}: {error}", source.display());
                failed += 1;
                continue;
            }
        };

        let decrypted = match decrypt_lua(&encrypted_bytes) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("asset_unpacker: failed to decrypt {}: {error}", source.display());
                failed += 1;
                continue;
            }
        };

        match fs::write(&destination, decrypted) {
            Ok(()) => {
                println!("Decrypted: {} -> {}", source.display(), destination.display());
                succeeded += 1;
            }
            Err(error) => {
                eprintln!("asset_unpacker: failed to write {}: {error}", destination.display());
                failed += 1;
            }
        }
    }

    println!("Finished: {succeeded} file(s) decrypted, {failed} failure(s)");
    if failed > 0 {
        return Err(format!("failed to process {failed} file(s)").into());
    }
    Ok(())
}
