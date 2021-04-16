//! Kernel cmdline parsing - utility functions
//!
//! NOTE(lucab): this is not a complete/correct cmdline parser, as it implements
//!  just enough logic to extract the platform ID value. In particular, it does not
//!  handle separator quoting/escaping, list of values, and merging of repeated
//!  flags. Logic is taken from Afterburn, please backport any bugfix there too:
//!  https://github.com/coreos/afterburn/blob/v4.1.0/src/util/cmdline.rs

use anyhow::{Context, Result};
use std::io::Read;
use std::{fs, io};

/// Platform key.
static CMDLINE_PLATFORM_FLAG: &str = "ignition.platform.id";

/// Read platform value from cmdline file.
pub(crate) fn read_id<T>(cmdline_path: T) -> Result<String>
where
    T: AsRef<str>,
{
    // open the cmdline file
    let fpath = cmdline_path.as_ref();
    let file =
        fs::File::open(fpath).with_context(|| format!("failed to open cmdline file {}", fpath))?;

    // read content
    let mut bufrd = io::BufReader::new(file);
    let mut contents = String::new();
    bufrd
        .read_to_string(&mut contents)
        .with_context(|| format!("failed to read cmdline file {}", fpath))?;

    // lookup flag by key name
    match find_flag_value(CMDLINE_PLATFORM_FLAG, &contents) {
        Some(platform) => {
            log::trace!("found platform id: {}", platform);
            Ok(platform)
        }
        None => anyhow::bail!(
            "could not find flag '{}' in {}",
            CMDLINE_PLATFORM_FLAG,
            fpath
        ),
    }
}

/// Find OEM ID flag value in cmdline string.
fn find_flag_value(flagname: &str, cmdline: &str) -> Option<String> {
    // split content into elements and keep key-value tuples only.
    let params: Vec<(&str, &str)> = cmdline
        .split(' ')
        .filter_map(|s| {
            let kv: Vec<&str> = s.splitn(2, '=').collect();
            match kv.len() {
                2 => Some((kv[0], kv[1])),
                _ => None,
            }
        })
        .collect();

    // find the oem flag
    for (key, val) in params {
        if key != flagname {
            continue;
        }
        let bare_val = val.trim();
        if !bare_val.is_empty() {
            return Some(bare_val.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_find_flag() {
        let flagname = "ignition.platform.id";
        let tests = vec![
            ("", None),
            ("foo=bar", None),
            ("ignition.platform.id", None),
            ("ignition.platform.id=", None),
            ("ignition.platform.id=\t", None),
            ("ignition.platform.id=ec2", Some("ec2".to_string())),
            ("ignition.platform.id=\tec2", Some("ec2".to_string())),
            ("ignition.platform.id=ec2\n", Some("ec2".to_string())),
            ("foo=bar ignition.platform.id=ec2", Some("ec2".to_string())),
            ("ignition.platform.id=ec2 foo=bar", Some("ec2".to_string())),
        ];
        for (tcase, tres) in tests {
            let res = find_flag_value(flagname, tcase);
            assert_eq!(res, tres, "failed testcase: '{}'", tcase);
        }
    }
}
