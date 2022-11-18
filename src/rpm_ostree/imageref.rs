//! This is a copy of code from ostreedev/ostree-rs-ext to avoid
//! depending on that whole library.

use std::borrow::Cow;
use std::str::FromStr;

use anyhow::{anyhow, Result};

/// A backend/transport for OCI/Docker images.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    /// A remote Docker/OCI registry (`registry:` or `docker://`)
    Registry,
    /// A local OCI directory (`oci:`)
    OciDir,
    /// A local OCI archive tarball (`oci-archive:`)
    OciArchive,
    /// Local container storage (`containers-storage:`)
    ContainerStorage,
}

/// Combination of a remote image reference and transport.
///
/// For example,
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageReference {
    /// The storage and transport for the image
    pub transport: Transport,
    /// The image name (e.g. `quay.io/somerepo/someimage:latest`)
    pub name: String,
}

/// Policy for signature verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureSource {
    /// Fetches will use the named ostree remote for signature verification of the ostree commit.
    OstreeRemote(String),
    /// Fetches will defer to the `containers-policy.json`, but we make a best effort to reject `default: insecureAcceptAnything` policy.
    ContainerPolicy,
    /// NOT RECOMMENDED.  Fetches will defer to the `containers-policy.json` default which is usually `insecureAcceptAnything`.
    ContainerPolicyAllowInsecure,
}

/// Combination of a signature verification mechanism, and a standard container image reference.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OstreeImageReference {
    /// The signature verification mechanism.
    pub sigverify: SignatureSource,
    /// The container image reference.
    pub imgref: ImageReference,
}

impl TryFrom<&str> for Transport {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        Ok(match value {
            "registry" | "docker" => Self::Registry,
            "oci" => Self::OciDir,
            "oci-archive" => Self::OciArchive,
            "containers-storage" => Self::ContainerStorage,
            o => return Err(anyhow!("Unknown transport '{}'", o)),
        })
    }
}

impl TryFrom<&str> for ImageReference {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        let (transport_name, mut name) = value
            .split_once(':')
            .ok_or_else(|| anyhow!("Missing ':' in {}", value))?;
        let transport: Transport = transport_name.try_into()?;
        if name.is_empty() {
            return Err(anyhow!("Invalid empty name in {}", value));
        }
        if transport_name == "docker" {
            name = name
                .strip_prefix("//")
                .ok_or_else(|| anyhow!("Missing // in docker:// in {}", value))?;
        }
        Ok(Self {
            transport,
            name: name.to_string(),
        })
    }
}

impl FromStr for ImageReference {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for SignatureSource {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "ostree-image-signed" => Ok(Self::ContainerPolicy),
            "ostree-unverified-image" => Ok(Self::ContainerPolicyAllowInsecure),
            o => match o.strip_prefix("ostree-remote-image:") {
                Some(rest) => Ok(Self::OstreeRemote(rest.to_string())),
                _ => Err(anyhow!("Invalid signature source: {}", o)),
            },
        }
    }
}

impl FromStr for SignatureSource {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for OstreeImageReference {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        let (first, second) = value
            .split_once(':')
            .ok_or_else(|| anyhow!("Missing ':' in {}", value))?;
        let (sigverify, rest) = match first {
            "ostree-image-signed" => (SignatureSource::ContainerPolicy, Cow::Borrowed(second)),
            "ostree-unverified-image" => (
                SignatureSource::ContainerPolicyAllowInsecure,
                Cow::Borrowed(second),
            ),
            // Shorthand for ostree-unverified-image:registry:
            "ostree-unverified-registry" => (
                SignatureSource::ContainerPolicyAllowInsecure,
                Cow::Owned(format!("registry:{second}")),
            ),
            // This is a shorthand for ostree-remote-image with registry:
            "ostree-remote-registry" => {
                let (remote, rest) = second
                    .split_once(':')
                    .ok_or_else(|| anyhow!("Missing second ':' in {}", value))?;
                (
                    SignatureSource::OstreeRemote(remote.to_string()),
                    Cow::Owned(format!("registry:{rest}")),
                )
            }
            "ostree-remote-image" => {
                let (remote, rest) = second
                    .split_once(':')
                    .ok_or_else(|| anyhow!("Missing second ':' in {}", value))?;
                (
                    SignatureSource::OstreeRemote(remote.to_string()),
                    Cow::Borrowed(rest),
                )
            }
            o => {
                return Err(anyhow!("Invalid ostree image reference scheme: {}", o));
            }
        };
        let imgref = (&*rest).try_into()?;
        Ok(Self { sigverify, imgref })
    }
}

impl FromStr for OstreeImageReference {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::try_from(s)
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            // TODO once skopeo supports this, canonicalize as registry:
            Self::Registry => "docker://",
            Self::OciArchive => "oci-archive:",
            Self::OciDir => "oci:",
            Self::ContainerStorage => "containers-storage:",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for ImageReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.transport, self.name)
    }
}

impl std::fmt::Display for OstreeImageReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.sigverify {
            SignatureSource::OstreeRemote(r) => {
                write!(f, "ostree-remote-image:{}:{}", r, self.imgref)
            }
            SignatureSource::ContainerPolicy => write!(f, "ostree-image-signed:{}", self.imgref),
            SignatureSource::ContainerPolicyAllowInsecure => {
                write!(f, "ostree-unverified-image:{}", self.imgref)
            }
        }
    }
}
