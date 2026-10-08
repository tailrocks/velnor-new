use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::types::RunnerImageIdentityView;

const SHA256_PREFIX: &str = "sha256:";
const REQUIRED_ADMISSION_PROFILE: &str = "ubuntu-26.04-amd64";

pub(super) fn has_required_admission_profile(
    image: &RunnerImageIdentityView<'_>,
    expected_scale_set_name: &str,
) -> bool {
    image.profile == REQUIRED_ADMISSION_PROFILE
        && validate_image_profile(image, expected_scale_set_name).is_some()
}

pub(super) fn validate_image_profile(
    image: &RunnerImageIdentityView<'_>,
    expected_scale_set_name: &str,
) -> Option<SystemTime> {
    let runner_deadline = parse_utc_second(image.runner_requalify_by)?;
    let release_at = parse_utc_second(image.runner_release_published_at)?;
    if !safe_text(image.profile)
        || image.profile != "ubuntu-24.04-amd64"
        || !safe_text(image.scale_set_name)
        || expected_scale_set_name != "ubuntu-24.04-scale-set"
        || image.scale_set_name != expected_scale_set_name
        || image.platform != "linux/amd64"
        || !image_reference_matches(
            image.runner_image,
            "ghcr.io/actions/actions-runner@",
            image.runner_manifest_digest,
        )
        || !sha256_digest(image.runner_manifest_digest)
        || !sha256_digest(image.runner_index_digest)
        || !sha256_digest(image.runner_config_digest)
        || image.runner_os != "ubuntu24"
        || !safe_text(image.runner_release_version)
        || !safe_text(image.runner_release_published_at)
        || !safe_text(image.runner_requalify_by)
        || !image_reference_matches(
            image.dind_image,
            "docker.io/library/docker@",
            image.dind_manifest_digest,
        )
        || !sha256_digest(image.dind_manifest_digest)
        || !sha256_digest(image.dind_index_digest)
        || !sha256_digest(image.dind_config_digest)
        || !safe_text(image.dind_version)
        || !safe_text(image.dind_source)
        || !sha256_hex(image.dind_entrypoint_sha256)
        || release_at >= runner_deadline
    {
        return None;
    }
    Some(runner_deadline)
}

fn safe_text(value: &str) -> bool {
    !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn sha256_digest(value: &str) -> bool {
    value.strip_prefix(SHA256_PREFIX).is_some_and(sha256_hex)
}

fn image_reference_matches(reference: &str, prefix: &str, digest: &str) -> bool {
    reference.strip_prefix(prefix) == Some(digest)
}

fn sha256_hex(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_utc_second(value: &str) -> Option<SystemTime> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let year = i64::from(decimal(&bytes[0..4])?);
    let month = i64::from(decimal(&bytes[5..7])?);
    let day = i64::from(decimal(&bytes[8..10])?);
    let hour = i64::from(decimal(&bytes[11..13])?);
    let minute = i64::from(decimal(&bytes[14..16])?);
    let second = i64::from(decimal(&bytes[17..19])?);
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || hour > 23
        || minute > 59
        || second > 59
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    UNIX_EPOCH.checked_add(Duration::from_secs(u64::try_from(seconds).ok()?))
}

fn decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::{parse_utc_second, validate_image_profile};
    use crate::policy::RunnerImageIdentityView;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn image(deadline: &'static str) -> RunnerImageIdentityView<'static> {
        RunnerImageIdentityView {
            profile: "ubuntu-24.04-amd64",
            scale_set_name: "ubuntu-24.04-scale-set",
            platform: "linux/amd64",
            runner_image: "ghcr.io/actions/actions-runner@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            runner_manifest_digest: DIGEST,
            runner_index_digest: DIGEST,
            runner_config_digest: DIGEST,
            runner_os: "ubuntu24",
            runner_release_version: "2.338.0",
            runner_release_published_at: "2026-10-06T13:55:11Z",
            runner_requalify_by: deadline,
            dind_image: "docker.io/library/docker@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            dind_manifest_digest: DIGEST,
            dind_index_digest: DIGEST,
            dind_config_digest: DIGEST,
            dind_version: "29.8.2",
            dind_source: "docker-library/docker@0123456789abcdef0123456789abcdef01234567",
            dind_entrypoint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        }
    }

    #[test]
    fn parses_and_enforces_the_exact_rfc3339_profile_deadline() {
        let deadline = parse_utc_second("2026-11-05T13:55:11Z").expect("fixed UTC timestamp");
        assert_eq!(
            deadline.duration_since(UNIX_EPOCH).expect("after epoch"),
            Duration::from_secs(1_793_886_911)
        );
        let now = parse_utc_second("2026-10-08T00:00:00Z").expect("fixed UTC timestamp");
        assert_eq!(
            validate_image_profile(&image("2026-11-05T13:55:11Z"), "ubuntu-24.04-scale-set"),
            Some(deadline)
        );
        assert!(now < deadline);
    }

    #[test]
    fn rejects_malformed_or_mismatched_image_profiles() {
        assert_eq!(
            validate_image_profile(&image("2026-11-05"), "ubuntu-24.04-scale-set"),
            None
        );
        assert_eq!(
            validate_image_profile(&image("2026-11-05T13:55:11Z"), "ubuntu-26.04-scale-set"),
            None
        );
        let invalid_runner_ref = RunnerImageIdentityView {
            runner_image: "ghcr.io/actions/actions-runner@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef?mirror=untrusted",
            ..image("2026-11-05T13:55:11Z")
        };
        assert_eq!(
            validate_image_profile(&invalid_runner_ref, "ubuntu-24.04-scale-set"),
            None
        );
        let invalid_dind_ref = RunnerImageIdentityView {
            dind_image: "docker.io/library/docker@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef/extra",
            ..image("2026-11-05T13:55:11Z")
        };
        assert_eq!(
            validate_image_profile(&invalid_dind_ref, "ubuntu-24.04-scale-set"),
            None
        );
        assert_eq!(parse_utc_second("2026-02-29T00:00:00Z"), None);
        assert_eq!(parse_utc_second("2026-10-06T13:55:11+00:00"), None);
    }
}
