//! Logic for the `deadend` subcommand.

use super::ensure_user;
use anyhow::{Context, Result};
use clap::Subcommand;
use fn_error_context::context;
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

/// Absolute path to the MOTD fragments directory.
static MOTD_FRAGMENTS_DIR: &str = "/run/motd.d/";
/// Absolute path to the MOTD fragment with deadend state.
static DEADEND_MOTD_PATH: &str = "/run/motd.d/85-zincati-deadend.motd";

/// Subcommand `deadend-motd`.
#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Set deadend state, with given reason.
    #[command(name = "set")]
    Set {
        #[arg(long = "reason")]
        reason: String,
    },
    /// Unset deadend state.
    #[command(name = "unset")]
    Unset,
}

impl Cmd {
    /// `deadend-motd` subcommand entry point.
    #[context("failed to run `deadend-motd` subcommand")]
    pub(crate) fn run(self) -> Result<()> {
        ensure_user(
            "root",
            "deadend-motd subcommand must be run as `root` user, \
             and should be called by the Zincati agent process",
        )?;
        match self {
            Cmd::Set { reason } => refresh_motd_fragment(reason),
            Cmd::Unset => remove_motd_fragment(),
        }
    }
}

/// Refresh MOTD fragment with deadend reason.
fn refresh_motd_fragment(reason: String) -> Result<()> {
    // Avoid showing partially-written messages using tempfile and
    // persist (rename).
    let mut f = tempfile::Builder::new()
        .prefix(".deadend.")
        .suffix(".motd.partial")
        // Create the tempfile in the same directory as the final MOTD,
        // to ensure proper SELinux labels are applied to the tempfile
        // before renaming.
        .tempfile_in(MOTD_FRAGMENTS_DIR)
        .with_context(|| {
            format!(
                "failed to create temporary MOTD file under '{}'",
                MOTD_FRAGMENTS_DIR
            )
        })?;
    // Set correct permissions of the temporary file, before moving to
    // the destination (`tempfile` creates files with mode 0600).
    std::fs::set_permissions(f.path(), Permissions::from_mode(0o644)).with_context(|| {
        format!(
            "failed to set permissions of temporary MOTD file at '{}'",
            f.path().display()
        )
    })?;

    writeln!(
        f,
        "This release is a dead-end and will not further auto-update: {}",
        reason
    )
    .and_then(|_| f.flush())
    .with_context(|| format!("failed to write MOTD content to '{}'", f.path().display()))?;

    f.persist(DEADEND_MOTD_PATH)
        .with_context(|| format!("failed to persist MOTD fragment to '{}'", DEADEND_MOTD_PATH))?;
    Ok(())
}

/// Remove motd fragment file, if any.
fn remove_motd_fragment() -> Result<()> {
    if let Err(e) = std::fs::remove_file(DEADEND_MOTD_PATH) {
        if e.kind() != std::io::ErrorKind::NotFound {
            anyhow::bail!(
                "failed to remove MOTD fragment at '{}': {}",
                DEADEND_MOTD_PATH,
                e
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{CliCommand, CliOptions};
    use clap::Parser;

    #[test]
    fn test_deadend_motd_set() {
        {
            let missing_flag = vec!["zincati", "deadend-motd", "set"];
            let cli = CliOptions::try_parse_from(missing_flag);
            assert!(cli.is_err());
        }
        {
            let missing_reason = vec!["zincati", "deadend-motd", "set", "--reason"];
            let cli = CliOptions::try_parse_from(missing_reason);
            assert!(cli.is_err());
        }
        {
            let empty_reason = vec!["zincati", "deadend-motd", "set", "--reason", ""];
            let cli = CliOptions::try_parse_from(empty_reason).unwrap();
            if let CliCommand::DeadendMotd(Cmd::Set { reason }) = &cli.cmd {
                assert_eq!(reason, "");
            } else {
                panic!("unexpected result: {:?}", cli);
            }
        }
        {
            let reason_message = vec!["zincati", "deadend-motd", "set", "--reason", "foo"];
            let cli = CliOptions::try_parse_from(reason_message).unwrap();
            if let CliCommand::DeadendMotd(Cmd::Set { reason }) = &cli.cmd {
                assert_eq!(reason, "foo");
            } else {
                panic!("unexpected result: {:?}", cli);
            }
        }
    }

    #[test]
    fn test_deadend_motd_unset() {
        {
            let extra_flags = vec!["zincati", "deadend-motd", "unset", "--reason", "foo"];
            let cli = CliOptions::try_parse_from(extra_flags);
            assert!(cli.is_err());
        }
        {
            let unset = vec!["zincati", "deadend-motd", "unset"];
            let cli = CliOptions::try_parse_from(unset).unwrap();
            if !matches!(&cli.cmd, CliCommand::DeadendMotd(Cmd::Unset)) {
                panic!("unexpected result: {:?}", cli);
            }
        }
    }
}
