//! Closed APT delivery policy; data and lexical validation only.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Inputs for the fixed APT delivery workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AptDeliveryConfig {
    /// GitHub repository containing release artifacts (`owner/repository`).
    pub source_repository: String,
    /// GitHub repository owning the published feed (`owner/repository`).
    pub consumer_repository: String,
    /// Debian package name.
    pub package: String,
    /// Release binary basename.
    pub binary: String,
    /// Basename under `usr/share` containing packaged release identities.
    pub identity_directory: String,
    /// Exact package manifest schema identifier.
    pub manifest_schema: String,
    /// Repository-relative public signing keyring file.
    pub keyring: String,
    /// Exact signing key fingerprint: 40 uppercase hexadecimal characters.
    pub signer_fingerprint: String,
    /// APT Release origin label.
    pub origin: String,
    /// APT Release description.
    pub description: String,
    /// HTTPS feed origin, without credentials, path, query, or fragment.
    pub feed_url: String,
    /// Feed branch (defaults to `main`).
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Five-field numeric POSIX cron schedule.
    pub schedule: String,
    /// Repository-relative signer workflow under `.github/workflows/`.
    pub signer_workflow: String,
    /// Qualified lowercase GHCR repository, without a tag or digest.
    pub oci_image_repository: String,
    /// Workflow identity authorized to sign the source OCI image.
    pub oci_signer_workflow: String,
}

fn default_branch() -> String {
    "main".to_owned()
}

impl AptDeliveryConfig {
    /// Validate every input before workflow emission.
    /// # Errors
    /// Reports the config filename and exact `delivery.apt` key.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let checks = [
            ("source_repository", repository(&self.source_repository)),
            ("consumer_repository", repository(&self.consumer_repository)),
            ("package", package_name(&self.package)),
            ("binary", basename(&self.binary)),
            ("identity_directory", basename(&self.identity_directory)),
            ("manifest_schema", schema_identifier(&self.manifest_schema)),
            ("keyring", relative_path(&self.keyring)),
            ("signer_fingerprint", fingerprint(&self.signer_fingerprint)),
            ("origin", release_text(&self.origin)),
            ("description", release_text(&self.description)),
            ("feed_url", https_origin(&self.feed_url)),
            ("branch", crate::is_valid_branch_name(&self.branch)),
            ("schedule", cron(&self.schedule)),
            ("signer_workflow", workflow_path(&self.signer_workflow)),
            (
                "oci_image_repository",
                image_repository(&self.oci_image_repository),
            ),
            (
                "oci_signer_workflow",
                workflow_path(&self.oci_signer_workflow),
            ),
        ];
        for (field, valid) in checks {
            if !valid {
                return Err(ContractError::config(
                    file,
                    format!("delivery.apt.{field}"),
                    "invalid_apt_delivery_value",
                ));
            }
        }
        Ok(())
    }
}

fn repository(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && owner.len() <= 39
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && !owner.contains("--")
        && repo.len() <= 100
        && basename(repo)
}

fn image_repository(value: &str) -> bool {
    let Some(path) = value.strip_prefix("ghcr.io/") else {
        return false;
    };
    path.len() <= 255 && path.split('/').count() >= 2 && path.split('/').all(image_component)
}

fn image_component(value: &str) -> bool {
    let mut bytes = value.bytes().peekable();
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !bytes.next().is_some_and(alnum) {
        return false;
    }
    while let Some(byte) = bytes.next() {
        if alnum(byte) {
            continue;
        }
        match byte {
            b'-' => {
                while bytes.peek() == Some(&b'-') {
                    bytes.next();
                }
            }
            b'_' => {
                if bytes.peek() == Some(&b'_') {
                    bytes.next();
                }
            }
            b'.' => {}
            _ => return false,
        }
        if !bytes.next().is_some_and(alnum) {
            return false;
        }
    }
    true
}

fn package_name(value: &str) -> bool {
    value.len() >= 2
        && value.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+-.".contains(&b))
}

fn basename(value: &str) -> bool {
    !value.is_empty()
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && !value.contains("..")
}

fn relative_path(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        && value
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('-') && part != "." && part != "..")
}

fn schema_identifier(value: &str) -> bool {
    value.split('.').count() >= 2
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn fingerprint(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

fn release_text(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .,_-():".contains(&b))
}

fn https_origin(value: &str) -> bool {
    let Some(authority) = value.strip_prefix("https://") else {
        return false;
    };
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(h, p)| (h, Some(p)));
    let valid_host = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    valid_host
        && port.is_none_or(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n > 0)
        })
}

fn workflow_path(value: &str) -> bool {
    let Some(name) = value.strip_prefix(".github/workflows/") else {
        return false;
    };
    basename(name)
        && name
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| suffix == "yml")
        && name.len() > 4
}

fn cron(value: &str) -> bool {
    let fields: Vec<&str> = value.split(' ').collect();
    fields.len() == 5
        && fields
            .iter()
            .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)])
            .all(|(field, (min, max))| cron_field(field, min, max))
}

fn cron_field(value: &str, min: u32, max: u32) -> bool {
    value.split(',').all(|item| {
        let (range, step) = item
            .split_once('/')
            .map_or((item, None), |(r, s)| (r, Some(s)));
        if step.is_some_and(|s| !cron_number(s, 1, max - min + 1)) {
            return false;
        }
        if range == "*" {
            return true;
        }
        if let Some((start, end)) = range.split_once('-') {
            return cron_number(start, min, max)
                && cron_number(end, min, max)
                && start.parse::<u32>().ok() <= end.parse::<u32>().ok();
        }
        step.is_none() && cron_number(range, min, max)
    })
}

fn cron_number(value: &str, min: u32, max: u32) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u32>().is_ok_and(|n| (min..=max).contains(&n))
}

#[cfg(test)]
#[path = "delivery_apt_tests.rs"]
mod tests;
