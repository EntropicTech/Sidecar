use crate::{BUS, INTERFACE, OBJECT};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Connections are deliberately recreated for each transaction. A daemon restart
/// cannot leave the UI or CLI pinned to a stale unique bus owner.
pub fn call<T: DeserializeOwned>(method: &str, payload: &impl Serialize) -> Result<T> {
    let operation = crate::api::Operation::parse(method)?;
    let method = operation.as_str();
    let mut value = serde_json::to_value(payload)?;
    if method == "stop" && !value["expected_id"].is_null() {
        require_feature("guarded_stop")?;
    }
    let timeout = if operation.large() { 30 } else { 3 };
    let connection = zbus::blocking::connection::Builder::session()
        .context("No session D-Bus; run inside your Plasma session")?
        .method_timeout(std::time::Duration::from_secs(timeout))
        .build()
        .context("No session D-Bus; run inside your Plasma session")?;
    let proxy = zbus::blocking::Proxy::new(&connection, BUS, OBJECT, INTERFACE)?;
    let request_id = format!(
        "{}-{}-{}",
        std::process::id(),
        crate::model::monotonic_ns(),
        REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    value
        .as_object_mut()
        .context("RPC payload must be an object")?
        .insert("_request_id".into(), request_id.clone().into());
    let input = serde_json::to_string(&value)?;
    let output: String = proxy.call("Request", &(method, input)).with_context(||
        format!("Collector request '{method}' failed (request {request_id}). Start it with: systemctl --user start igpu-man.service"))?;
    serde_json::from_str(&output).context("Invalid collector response / incompatible API version")
}

pub fn require_feature(feature: &str) -> Result<()> {
    let capabilities:serde_json::Value=call("capabilities",&serde_json::json!({})).context("This operation requires an updated collector; restart igpu-man.service when no recording or trial is in progress")?;
    anyhow::ensure!(
        capabilities["features"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v.as_str() == Some(feature))),
        "Collector does not support required feature {feature}"
    );
    Ok(())
}

pub fn dashboard_status() -> Result<serde_json::Value> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(std::time::Duration::from_secs(2))
        .build()?;
    let bus = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let exists: bool = bus.call("NameHasOwner", &(crate::DASHBOARD_BUS))?;
    if !exists {
        return Ok(serde_json::json!({"running":false}));
    }
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        crate::DASHBOARD_BUS,
        crate::DASHBOARD_OBJECT,
        crate::DASHBOARD_INTERFACE,
    )?;
    let value: String = proxy.call("Status", &())?;
    Ok(serde_json::from_str(&value)?)
}

/// Compatibility fallback is only for reads; never retry a recording mutation.
pub fn recording_timeline(value: &serde_json::Value) -> Result<crate::model::RecordingTimeline> {
    let caps: serde_json::Value = call("capabilities", &serde_json::json!({}))?;
    if caps["features"].as_array().is_some_and(|v| {
        v.iter()
            .any(|v| v.as_str() == Some("compact_recording_timeline"))
    }) {
        return call("timeline", value);
    }
    let samples: Vec<crate::model::Snapshot> = call("samples", value)?;
    let frames: Vec<crate::model::FrameLog> = call("frames", value)?;
    Ok(crate::model::RecordingTimeline {
        samples: samples.into_iter().map(Into::into).collect(),
        frames: frames.into_iter().map(Into::into).collect(),
    })
}
