#[cfg(test)]
mod tests {
    use crate::catalog::{
        JACTIONLINT_DIGESTS, jactionlint_archive_sha256, jactionlint_executable_sha256,
    };
    use crate::catalog::{JACTIONLINT_VERSION, validate_concrete_exact_version};

    const PLATFORMS: [&str; 4] = ["linux-arm64", "linux-x64", "macos-arm64", "macos-x64"];

    fn is_sha256(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn the_catalog_pin_has_every_runner_platform() {
        for platform in PLATFORMS {
            assert!(
                jactionlint_executable_sha256(JACTIONLINT_VERSION, platform).is_some(),
                "bump JACTIONLINT_VERSION together with its {platform} digests"
            );
            assert!(jactionlint_archive_sha256(JACTIONLINT_VERSION, platform).is_some());
        }
    }

    #[test]
    fn rows_are_exact_sorted_unique_and_lowercase_sha256() {
        let mut previous: Option<(Vec<u64>, &str)> = None;
        for row in JACTIONLINT_DIGESTS {
            validate_concrete_exact_version("jactionlint", row.version).expect("exact version");
            assert!(PLATFORMS.contains(&row.platform), "{}", row.platform);
            assert!(is_sha256(row.archive_sha256), "{}", row.asset);
            assert!(is_sha256(row.executable_sha256), "{}", row.asset);
            assert_ne!(row.archive_sha256, row.executable_sha256, "{}", row.asset);
            assert!(
                row.asset
                    .starts_with(&format!("jactionlint_{}_", row.version)),
                "{}",
                row.asset
            );
            let key: Vec<u64> = row
                .version
                .split('.')
                .map(|part| part.parse().expect("numeric"))
                .collect();
            if let Some(previous) = &previous {
                assert!(
                    (&previous.0, previous.1) < (&key, row.platform),
                    "{} {} sorted and unique",
                    row.version,
                    row.platform
                );
            }
            previous = Some((key, row.platform));
        }
        assert_eq!(
            JACTIONLINT_DIGESTS.last().map(|row| row.version),
            Some(JACTIONLINT_VERSION),
            "the newest rows are the catalog pin"
        );
    }

    #[test]
    fn unknown_versions_and_platforms_have_no_digest() {
        assert_eq!(jactionlint_executable_sha256("2.2.0", "linux-x64"), None);
        assert_eq!(
            jactionlint_executable_sha256(JACTIONLINT_VERSION, "windows-x64"),
            None
        );
        assert_eq!(jactionlint_archive_sha256("2.3", "linux-x64"), None);
    }
}
