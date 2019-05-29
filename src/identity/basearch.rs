//! Fedora CoreOS base architecture.

use cfg_if::cfg_if;
use failure::Fallible;

// TODO(lucab): consider sourcing this directly from somewhere in OS rootfs.

// Map Rust target to Fedora basearch.
cfg_if! {
    if #[cfg(target_arch = "aarch64")] {
        static BASEARCH: &str = "aarch64";
    } else if #[cfg(target_arch = "powerpc64le")] {
        static BASEARCH: &str = "ppc64le";
    } else if #[cfg(target_arch = "x86_64")] {
        static BASEARCH: &str = "x86_64";
    } else {
        static BASEARCH: &str = "unsupported";
    }
}

/// Fetch base architecture value.
pub(crate) fn read_basearch() -> Fallible<String> {
    if BASEARCH == "unsupported" {
        // For forward compatibility, we log a message but keep going.
        log::error!("unsupported base architecture");
    }

    Ok(BASEARCH.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn basic_basearch() {
        let label = super::read_basearch().unwrap();
        assert!(!label.is_empty());
    }
}
