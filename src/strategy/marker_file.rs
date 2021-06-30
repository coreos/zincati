//! Strategy for local marker file-based updates.

use anyhow::{Context, Error, Result};
use fn_error_context::context;
use futures::future;
use futures::prelude::*;
use log::trace;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::PermissionsExt;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tokio_fs;

/// Struct to parse finalization marker file's JSON content into.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinalizationMarker {
    /// Unix timestamp of expiry time.
    allow_until: Option<u64>,
}

/// Strategy for immediate updates.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StrategyMarkerFile {}

impl StrategyMarkerFile {
    /// Strategy label/name.
    pub const LABEL: &'static str = "marker_file";
    /// Local filesystem path to finalization marker file.
    pub const FINALIZATION_MARKER_FILE_PATH: &'static str =
        "/var/lib/zincati/admin/strategy/marker_file/allowfinalize.json";

    /// Check if finalization is allowed.
    pub(crate) fn can_finalize(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        Box::pin(Self::marker_file_allow_finalization(
            Self::FINALIZATION_MARKER_FILE_PATH,
        ))
    }

    /// Try to report steady state.
    pub(crate) fn report_steady(&self) -> Pin<Box<dyn Future<Output = Result<bool, Error>>>> {
        trace!("marker_file strategy, report steady: {}", true);

        let res = future::ok(true);
        Box::pin(res)
    }

    /// Asynchronous helper function that returns a future indicating whether
    /// finalization is allowed, depending on the presence of a marker file.
    async fn marker_file_allow_finalization(
        finalization_marker_path: &'static str,
    ) -> Result<bool> {
        if !verify_file_metadata(finalization_marker_path).await? {
            return Ok(false);
        }

        if is_expired(finalization_marker_path).await? {
            return Ok(false);
        }

        Ok(true)
    }
}

/// Verify that finalization marker file exists, is a regular file,
/// and has the correct permissions.
#[context("failed to verify finalization marker file metadata")]
async fn verify_file_metadata(path: &str) -> Result<bool> {
    let attr = tokio_fs::metadata(path).await;
    let attr = match attr {
        Ok(attr) => attr,
        // If `path` doesn't exist, return false early.
        Err(_) => return Ok(false),
    };

    if !attr.is_file() {
        anyhow::bail!("file is not regular file");
    }

    let mode = attr.permissions().mode();
    if mode & 0o2 != 0 {
        anyhow::bail!("file should not be writable by other");
    }

    Ok(true)
}

/// Check whether the finalization marker file has expired, if `allowUntil` key
/// exists.
async fn is_expired(path: &'static str) -> Result<bool> {
    match parse_expiry_timestamp(path).await? {
        Some(expiry_timestamp) => {
            // We can `unwrap()` since we're certain `UNIX_EPOCH` is in the past.
            let current_timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if current_timestamp >= expiry_timestamp {
                Ok(true)
            } else {
                Ok(false)
            }
        }
        None => Ok(false),
    }
}

#[context("failed to parse expiry timestamp from marker file")]
async fn parse_expiry_timestamp(path: &'static str) -> Result<Option<u64>> {
    let marker_json: Result<FinalizationMarker> = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let json = serde_json::from_reader(reader).context("failed to parse JSON content")?;
        Ok(json)
    })
    .await?;

    Ok(marker_json?.allow_until)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::io::BufWriter;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio::runtime as rt;

    #[test]
    fn test_marker_file_allow_finalization() {
        lazy_static::lazy_static! {
            static ref TEMPDIR_MARKER_FILE_PATH: String = {
                let p: PathBuf = tempdir().unwrap().into_path().join("allowfinalize.json");
                p.into_os_string().into_string().unwrap()
            };
        }
        let json = json!({});
        let f = fs::File::create(&*TEMPDIR_MARKER_FILE_PATH).unwrap();
        serde_json::to_writer(BufWriter::new(f), &json).unwrap();

        // This should pass since default file permissions are 644 and we don't check
        // for ownership by root.
        let runtime = rt::Runtime::new().unwrap();
        let can_finalize =
            StrategyMarkerFile::marker_file_allow_finalization(&*TEMPDIR_MARKER_FILE_PATH);
        let can_finalize = runtime.block_on(can_finalize).unwrap();
        assert!(can_finalize);

        // Set permissions to writable by other; expect an error.
        fs::set_permissions(
            &*TEMPDIR_MARKER_FILE_PATH,
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        let can_finalize =
            StrategyMarkerFile::marker_file_allow_finalization(&*TEMPDIR_MARKER_FILE_PATH);
        runtime
            .block_on(can_finalize)
            .expect_err("file with incorrect permissions unexpectedly allowed finalization");
    }

    #[test]
    fn test_parse_finalization_marker() {
        lazy_static::lazy_static! {
            static ref TEMPDIR_MARKER_FILE_PATH: String = {
                let p: PathBuf = tempdir().unwrap().into_path().join("allowfinalize.json");
                p.into_os_string().into_string().unwrap()
            };
        }
        // 1619640863 is Apr 28 2021 20:14:23 UTC.
        // Expect this to be expired.
        let json = json!({
            "allowUntil": 1619640863
        });
        let f = fs::File::create(&*TEMPDIR_MARKER_FILE_PATH).unwrap();
        serde_json::to_writer(BufWriter::new(f), &json).unwrap();
        let expired = is_expired(&*TEMPDIR_MARKER_FILE_PATH);
        let runtime = rt::Runtime::new().unwrap();
        let expired = runtime.block_on(expired).unwrap();
        assert_eq!(expired, true);

        // Expect timepstamp with value `u64::MAX` to not be expired.
        let json = json!({ "allowUntil": u64::MAX });
        let f = fs::File::create(&*TEMPDIR_MARKER_FILE_PATH).unwrap();
        serde_json::to_writer(BufWriter::new(f), &json).unwrap();
        let expired = is_expired(&*TEMPDIR_MARKER_FILE_PATH);
        let expired = runtime.block_on(expired).unwrap();
        assert_eq!(expired, false);

        // If no `allowUntil` field, marker file should not expire.
        let json = json!({});
        let f = fs::File::create(&*TEMPDIR_MARKER_FILE_PATH).unwrap();
        serde_json::to_writer(BufWriter::new(f), &json).unwrap();
        let expired = is_expired(&*TEMPDIR_MARKER_FILE_PATH);
        let expired = runtime.block_on(expired).unwrap();
        assert_eq!(expired, false);

        // Improper JSON.
        let json = "allowUntil=1619640863";
        fs::write(&*TEMPDIR_MARKER_FILE_PATH, json).unwrap();
        let expired = is_expired(&*TEMPDIR_MARKER_FILE_PATH);
        runtime
            .block_on(expired)
            .expect_err("improper JSON unexpectedly parsed without error");
    }
}
