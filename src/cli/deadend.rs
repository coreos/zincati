//! Logic for the `deadend` subcommand.

use super::ensure_user;
use failure::{bail, Fallible, ResultExt};
use std::fs::Permissions;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use structopt::StructOpt;

/// Absolute path to the MOTD fragments directory.
static MOTD_FRAGMENTS_DIR: &str = "/run/motd.d/";
/// Absolute path to the MOTD fragment with deadend state.
static DEADEND_MOTD_PATH: &str = "/run/motd.d/85-zincati-deadend.motd";

/// Subcommand `deadend-motd`.
#[derive(Debug, StructOpt)]
pub enum Cmd {
    /// Set deadend state, with given reason.
    #[structopt(name = "set")]
    Set {
        #[structopt(long = "reason")]
        reason: String,
    },
    /// Unset deadend state.
    #[structopt(name = "unset")]
    Unset,
}

impl Cmd {
    /// `deadend-motd` subcommand entry point.
    pub(crate) fn run(self) -> Fallible<()> {
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
fn refresh_motd_fragment(reason: String) -> Fallible<()> {
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
        reason
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
    Ok(())
}

/// Remove motd fragment file, if any.
fn remove_motd_fragment() -> Fallible<()> {
    if let Err(e) = std::fs::remove_file(DEADEND_MOTD_PATH) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{CliCommand, CliOptions};
    use structopt::StructOpt;

    #[test]
    fn test_deadend_motd_set() {
        {
            let missing_flag = vec!["zincati", "deadend-motd", "set"];
            let cli = CliOptions::from_iter_safe(missing_flag);
            assert!(cli.is_err());
        }
        {
            let missing_reason = vec!["zincati", "deadend-motd", "set", "--reason"];
            let cli = CliOptions::from_iter_safe(missing_reason);
            assert!(cli.is_err());
        }
        {
            let mut is_ok = false;
            let empty_reason = vec!["zincati", "deadend-motd", "set", "--reason", ""];
            let cli = CliOptions::from_iter_safe(empty_reason).unwrap();
            if let CliCommand::DeadendMotd(cmd) = &cli.cmd {
                if let Cmd::Set { reason } = cmd {
                    assert_eq!(reason, "");
                    is_ok = true;
                }
            }
            if !is_ok {
                panic!("unexpected result: {:?}", cli);
            }
        }
        {
            let mut is_ok = false;
            let reason_message = vec!["zincati", "deadend-motd", "set", "--reason", "foo"];
            let cli = CliOptions::from_iter_safe(reason_message).unwrap();
            if let CliCommand::DeadendMotd(cmd) = &cli.cmd {
                if let Cmd::Set { reason } = cmd {
                    assert_eq!(reason, "foo");
                    is_ok = true;
                }
            }
            if !is_ok {
                panic!("unexpected result: {:?}", cli);
            }
        }
    }

    #[test]
    fn test_deadend_motd_unset() {
        {
            let extra_flags = vec!["zincati", "deadend-motd", "unset", "--reason", "foo"];
            let cli = CliOptions::from_iter_safe(extra_flags);
            assert!(cli.is_err());
        }
        {
            let mut is_ok = false;
            let unset = vec!["zincati", "deadend-motd", "unset"];
            let cli = CliOptions::from_iter_safe(unset).unwrap();
            if let CliCommand::DeadendMotd(cmd) = &cli.cmd {
                if let Cmd::Unset = cmd {
                    is_ok = true;
                }
            }
            if !is_ok {
                panic!("unexpected result: {:?}", cli);
            }
        }
    }
}
