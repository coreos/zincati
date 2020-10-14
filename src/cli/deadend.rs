//! Logic for the `deadend` subcommand.

use failure::{bail, Fallible, ResultExt};
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

/// Deadend subcommand entry point.
pub(crate) fn run_deadend(reason: Option<String>) -> Fallible<()> {
    if reason.is_some() {
        // Avoid showing partially-written messages using tempfile and
        // persist (rename).
        let mut f = tempfile::Builder::new()
            .prefix(".deadend.")
            .suffix(".motd.partial")
            // Create the tempfile in the same directory as the final MOTD,
            // to ensure proper SELinux labels are applied to the tempfile
            // before renaming.
            .tempfile_in("/run/motd.d")
            .with_context(|e| format!("failed to create temporary MOTD file: {}", e))?;
        // Set correct permissions of the temporary file, before moving to
        // the destination (`tempfile` creates files with mode 0600).
        std::fs::set_permissions(f.path(), Permissions::from_mode(0o644))
            .with_context(|e| format!("failed to set permissions of temporary MOTD file: {}", e))?;

        if let Some(reason) = reason {
            writeln!(
                f,
                "This release is a dead-end and won't auto-update: {}",
                reason
            )
            .with_context(|e| format!("failed to write MOTD: {}", e))?;
        }

        f.persist("/run/motd.d/85-zincati-deadend.motd")
            .with_context(|e| format!("failed to persist temporary MOTD file: {}", e))?;
    } else if let Err(e) = std::fs::remove_file("/run/motd.d/85-zincati-deadend.motd") {
        if e.kind() != std::io::ErrorKind::NotFound {
            bail!("failed to remove dead-end release info file: {}", e);
        }
    }
    Ok(())
}
