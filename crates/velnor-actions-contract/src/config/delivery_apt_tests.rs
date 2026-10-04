//! Adversarial APT delivery configuration tests.
use super::AptDeliveryConfig;

fn config() -> AptDeliveryConfig {
    AptDeliveryConfig {
        source_repository: "tailrocks/velnor-new".to_owned(),
        consumer_repository: "tailrocks/apt".to_owned(),
        package: "velnor".to_owned(),
        binary: "velnor".to_owned(),
        identity_directory: "identity".to_owned(),
        manifest_schema: "velnor.package-release.v1".to_owned(),
        keyring: "keys/release.gpg".to_owned(),
        signer_fingerprint: "0123456789ABCDEF0123456789ABCDEF01234567".to_owned(),
        origin: "Tailrocks".to_owned(),
        description: "Tailrocks APT packages".to_owned(),
        feed_url: "https://apt.tailrocks.com".to_owned(),
        branch: "main".to_owned(),
        schedule: "17 */6 * * *".to_owned(),
        signer_workflow: ".github/workflows/sign.yml".to_owned(),
        oci_image_repository: "ghcr.io/tailrocks/velnor-job-ubuntu".to_owned(),
        oci_signer_workflow: ".github/workflows/publish-oci.yml".to_owned(),
    }
}

#[test]
fn closed_schema_and_branch_default() {
    let mut value = serde_json::to_value(config()).expect("serialize config");
    value.as_object_mut().expect("object").remove("branch");
    let decoded: AptDeliveryConfig = serde_json::from_value(value.clone()).expect("decode");
    assert_eq!(decoded.branch, "main");
    assert!(decoded.validate("config.toml").is_ok());
    for field in ["oci_image_repository", "oci_signer_workflow"] {
        let mut missing = value.clone();
        missing.as_object_mut().expect("object").remove(field);
        assert!(serde_json::from_value::<AptDeliveryConfig>(missing).is_err());
    }
    for field in ["run", "template", "uses", "shell"] {
        value[field] = serde_json::json!("arbitrary");
        assert!(serde_json::from_value::<AptDeliveryConfig>(value.clone()).is_err());
        value.as_object_mut().expect("object").remove(field);
    }
}

#[test]
fn hostile_values_report_exact_key() {
    let cases = [
        ("source_repository", "owner/repo/extra"),
        ("consumer_repository", "${{ github.repository }}"),
        ("package", "velnor;id"),
        ("binary", "../velnor"),
        ("identity_directory", "identities/../outside"),
        ("manifest_schema", "/schema.json"),
        ("keyring", "keys\\release.gpg"),
        (
            "signer_fingerprint",
            "0123456789abcdef0123456789abcdef01234567",
        ),
        ("origin", "Origin\nSuite: hostile"),
        ("description", "$(id)"),
        ("feed_url", "https://good.example@evil.example"),
        ("branch", "refs/../main"),
        ("schedule", "0 24 * * *"),
        ("signer_workflow", ".github/workflows/../sign.yml"),
        ("oci_signer_workflow", ".github/workflows/sign.yaml"),
        ("oci_image_repository", "ghcr.io/tailrocks/velnor:latest"),
        (
            "oci_image_repository",
            "ghcr.io/tailrocks/velnor@sha256:abc",
        ),
        ("oci_image_repository", "ghcr.io/tailrocks/VelNor"),
        ("oci_image_repository", "ghcr.io/tailrocks"),
        ("oci_image_repository", "ghcr.io/tailrocks/../velnor"),
        ("oci_image_repository", "ghcr.io/user:pass@tailrocks/velnor"),
        ("oci_image_repository", "ghcr.io/tailrocks/velnor__"),
        ("oci_image_repository", "docker.io/tailrocks/velnor"),
    ];
    for (field, raw) in cases {
        let mut value = serde_json::to_value(config()).expect("serialize config");
        value[field] = serde_json::json!(raw);
        let decoded: AptDeliveryConfig = serde_json::from_value(value).expect("typed config");
        let error = decoded.validate("config.toml").expect_err("hostile value");
        assert!(
            error
                .to_string()
                .contains(&format!("delivery.apt.{field}:")),
            "{error}"
        );
    }
}

#[test]
fn feed_and_cron_are_structured() {
    for raw in [
        "http://example.com",
        "https://example.com/",
        "https://example.com?q=x",
        "https://example.com#x",
        "https://example.com:0",
        "https://-example.com",
        "https://example.com\n",
    ] {
        let mut cfg = config();
        cfg.feed_url = raw.to_owned();
        assert!(cfg.validate("config.toml").is_err(), "{raw}");
    }
    for raw in [
        "* * * *",
        "* * * * * *",
        "@daily",
        "0 0 * JAN *",
        "*/0 * * * *",
        "0 0 31-1 * *",
        "0 0 * * 7",
        "0  0 * * *",
    ] {
        let mut cfg = config();
        cfg.schedule = raw.to_owned();
        assert!(cfg.validate("config.toml").is_err(), "{raw}");
    }
    for raw in ["0 0 * * *", "1,2 0-23/2 1-31 1-12 0-6", "*/5 * * * *"] {
        let mut cfg = config();
        cfg.schedule = raw.to_owned();
        assert!(cfg.validate("config.toml").is_ok(), "{raw}");
    }
}
