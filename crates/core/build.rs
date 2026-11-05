//! Reproducible Rust-source/dependency identity; no wall clock or absolute paths.
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn sources(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            sources(&path, files)?;
        } else if kind.is_file()
            && (path.extension().is_some_and(|s| s == "rs")
                || path.file_name().is_some_and(|s| s == "Cargo.toml"))
        {
            files.push(path);
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("Missing workspace root")?;
    let mut files = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    for entry in fs::read_dir(root.join("crates"))? {
        let path = entry?.path();
        if path.is_dir() {
            for name in ["Cargo.toml", "build.rs"] {
                let file = path.join(name);
                if file.is_file() {
                    files.push(file);
                }
            }
            let src = path.join("src");
            if src.is_dir() {
                sources(&src, &mut files)?;
            }
        }
    }
    let vendor = root.join("vendor");
    if vendor.is_dir() {
        sources(&vendor, &mut files)?;
    }
    files.sort();
    println!("cargo:rerun-if-changed={}", root.join("crates").display());
    println!("cargo:rerun-if-changed={}", vendor.display());
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    {
        let input = child.stdin.as_mut().ok_or("Missing hashing input")?;
        for path in files {
            println!("cargo:rerun-if-changed={}", path.display());
            let data = fs::read(&path)?;
            let name = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("Source paths must be UTF-8")?;
            input.write_all(&(name.len() as u64).to_le_bytes())?;
            input.write_all(name.as_bytes())?;
            input.write_all(&(data.len() as u64).to_le_bytes())?;
            input.write_all(&data)?;
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("sha256sum failed".into());
    }
    let output = String::from_utf8(output.stdout)?;
    let hash = output
        .split_whitespace()
        .next()
        .ok_or("Missing source digest")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid source digest".into());
    }
    let rustc = Command::new(env::var("RUSTC")?).arg("--version").output()?;
    if !rustc.status.success() {
        return Err("rustc version query failed".into());
    }
    println!("cargo:rustc-env=IGPU_MAN_SOURCE_ID={hash}");
    println!(
        "cargo:rustc-env=IGPU_MAN_RUSTC={}",
        String::from_utf8(rustc.stdout)?.trim()
    );
    println!("cargo:rustc-env=IGPU_MAN_PROFILE={}", env::var("PROFILE")?);
    println!(
        "cargo:rustc-env=IGPU_MAN_TARGET_FEATURES={}",
        env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default()
    );
    println!("cargo:rustc-env=IGPU_MAN_TARGET={}", env::var("TARGET")?);
    Ok(())
}
