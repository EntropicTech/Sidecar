use anyhow::{bail, Context, Result};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

pub fn read_text_limited(path: &Path, limit: u64) -> Result<String> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("Input exceeds the {} byte limit", limit);
    }
    String::from_utf8(bytes).context("Input is not valid UTF-8")
}

pub fn atomic_private_write(path: &Path, content: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .context("Configuration path needs a parent directory")?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    // tempfile uses unpredictable names and exclusive creation, so an old temp
    // file or symlink cannot redirect a save. The original survives failed writes.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temporary.write_all(content)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    // The rename is already committed. Report durability degradation without
    // incorrectly telling callers it is safe to roll back an unapplied write.
    if let Err(error) = File::open(parent).and_then(|file| file.sync_all()) {
        crate::logging::event(
            "warn",
            "config.directory_sync_failed",
            serde_json::json!({"io_kind":format!("{:?}",error.kind()),"applied":true}),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_are_private_atomic_and_do_not_follow_old_temp_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let untouched = dir.path().join("untouched");
        fs::write(&untouched, "keep").unwrap();
        std::os::unix::fs::symlink(&untouched, config.with_extension("toml.tmp")).unwrap();
        atomic_private_write(&config, b"first").unwrap();
        atomic_private_write(&config, b"second").unwrap();
        assert_eq!(fs::read_to_string(&untouched).unwrap(), "keep");
        assert_eq!(fs::read_to_string(&config).unwrap(), "second");
        assert_eq!(
            fs::metadata(config).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn bounded_reads_reject_large_or_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input");
        fs::write(&path, b"12345").unwrap();
        assert!(read_text_limited(&path, 4).is_err());
        fs::write(&path, [0xff]).unwrap();
        assert!(read_text_limited(&path, 4).is_err());
    }
}
