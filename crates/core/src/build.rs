//! Constant build metadata; hashing the running executable is explicit diagnostics only.
use anyhow::{Context, Result};
use serde_json::{json, Value};
pub const SOURCE_ID: &str = env!("IGPU_MAN_SOURCE_ID");
pub const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\nsource_sha256 ",
    env!("IGPU_MAN_SOURCE_ID"),
    "\n",
    env!("IGPU_MAN_RUSTC"),
    "\ntarget ",
    env!("IGPU_MAN_TARGET")
);
pub fn metadata() -> Value {
    json!({"version":env!("CARGO_PKG_VERSION"),"source_sha256":SOURCE_ID,"rustc":env!("IGPU_MAN_RUSTC"),"target":env!("IGPU_MAN_TARGET"),"profile":env!("IGPU_MAN_PROFILE"),"target_features":env!("IGPU_MAN_TARGET_FEATURES")})
}
pub fn executable_hash() -> Result<String> {
    let result = std::process::Command::new("sha256sum")
        .arg(format!("/proc/{}/exe", std::process::id()))
        .output()
        .context("Cannot hash running executable")?;
    anyhow::ensure!(result.status.success(), "Running executable hashing failed");
    let text = String::from_utf8(result.stdout)?;
    let hash = text
        .split_whitespace()
        .next()
        .context("Missing executable digest")?;
    anyhow::ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid executable digest"
    );
    Ok(hash.into())
}
