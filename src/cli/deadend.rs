//! Logic for the `deadend` subcommand.

use failure::{bail, Fallible, ResultExt};
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

/// Absolute path to the MOTD fragments directory.
static MOTD_FRAGMENTS_DIR: &str = "/run/motd.d/";
/// Absolute path to the MOTD fragment with deadend state.
static DEADEND_MOTD_PATH: &str = "/run/motd.d/85-zincati-deadend.motd";

/// Deadend subcommand entry point.
pub(crate) fn run_deadend(reason: Option<String>) -> Fallible<()> {
    if let Some(content) = reason {
        // Avoid showing partially-written messages using tempfile and
        // persist (rename).
        let mut f = tempfile::Builder::new()
            .prefix(".deadend.")
            .suffix(".motd.partial")
            // Create the tempfile in the same directory as the final MOTD,
            // to ensure proper SELinux labels are applied to the tempfile
            // before renaming.
            .tempfile_in(MOTD_FRAGMENTS_DIR)
            .context(format!(
                "failed to create temporary MOTD file under '{}'",
                MOTD_FRAGMENTS_DIR
            ))?;
        // Set correct permissions of the temporary file, before moving to
        // the destination (`tempfile` creates files with mode 0600).
        std::fs::set_permissions(f.path(), Permissions::from_mode(0o644)).context(format!(
            "failed to set permissions of temporary MOTD file at '{}'",
            f.path().display()
        ))?;

        writeln!(
            f,
            "This release is a dead-end and will not further auto-update: {}",
            content
        )
        .and_then(|_| f.flush())
        .context(format!(
            "failed to write MOTD content to '{}'",
            f.path().display()
        ))?;

        f.persist(DEADEND_MOTD_PATH).context(format!(
            "failed to persist MOTD fragment to '{}'",
            DEADEND_MOTD_PATH
        ))?;
    } else if let Err(e) = std::fs::remove_file(DEADEND_MOTD_PATH) {
        if e.kind() != std::io::ErrorKind::NotFound {
            bail!(
                "failed to remove MOTD fragment at '{}': {}",
                DEADEND_MOTD_PATH,
                e
            );
        }
    }
    Ok(())
}
