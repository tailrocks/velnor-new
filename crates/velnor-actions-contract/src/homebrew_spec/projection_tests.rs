use super::super::tests::*;

#[test]
fn canonical_spec_digest_covers_template_executable_policy_and_rejects_injection() {
    let baseline = spec();
    let digest = baseline.digest().expect("digest");
    let mut changed = baseline.clone();
    changed.auxiliaries[0].files[0].destination = "other/place".to_owned();
    assert_ne!(digest, changed.digest().expect("changed digest"));

    changed.description = "safe\"\nclass Evil < Formula".to_owned();
    assert!(changed.validate().is_err());
    changed.description = baseline.description.clone();
    changed.channels[0].destination = "Formula/../evil.rb".to_owned();
    assert!(changed.validate().is_err());
    changed = baseline;
    changed.channels[0].conflicts.push("jackin".to_owned());
    assert!(changed.validate().is_err());
}

#[test]
fn projection_selects_exact_jackin_payloads_bins_and_targets() {
    let policy = spec();
    let projection = validate_homebrew_projection(&policy, &plan(PackageReleaseChannel::Preview))
        .expect("projection");
    assert_eq!(projection.selected_payloads().len(), 6);
    assert_eq!(
        projection
            .selected_payloads()
            .iter()
            .filter(|payload| payload.role() == HomebrewPayloadRole::Host)
            .count(),
        4
    );
    assert_eq!(
        projection
            .selected_payloads()
            .iter()
            .filter(|payload| payload.role() == HomebrewPayloadRole::Auxiliary)
            .count(),
        2
    );
    assert_eq!(policy.host.bins.len(), 3);
    assert!(
        projection
            .selected_payloads()
            .iter()
            .all(|payload| payload.asset_name().ends_with(".tar.gz"))
    );

    let mut changed = policy.clone();
    changed.host.product = "missing".to_owned();
    assert!(validate_homebrew_projection(&changed, &plan(PackageReleaseChannel::Preview)).is_err());
    changed = policy;
    changed.auxiliaries[0].product = "jackin".to_owned();
    assert!(validate_homebrew_projection(&changed, &plan(PackageReleaseChannel::Preview)).is_err());
}

#[test]
fn qualified_catalog_projects_x86_linux_capsule_to_amd64_libexec_path() {
    const CATALOG: &str =
        include_str!("../../../../.velnor/qualification/homebrew/formula-spec.json");
    let catalog: HomebrewFormulaSpec = serde_json::from_str(CATALOG).expect("catalog spec");
    assert_eq!(
        catalog.canonical_bytes().expect("canonical spec"),
        CATALOG.as_bytes()
    );

    let projection = validate_homebrew_projection(&catalog, &plan(PackageReleaseChannel::Preview))
        .expect("qualified Jackin projection");
    let x86_payload = projection
        .selected_payloads()
        .iter()
        .find(|payload| {
            payload.product() == "jackin-capsule" && payload.target() == "x86_64-unknown-linux-gnu"
        })
        .expect("x86 Linux capsule payload");
    assert_eq!(
        x86_payload.asset_name(),
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert_eq!(x86_payload.members(), &[String::from("jackin-capsule")]);

    let release = literals(&catalog, &projection);
    let formula = String::from_utf8(
        render_homebrew_formula(&catalog, &release).expect("render qualified formula"),
    )
    .expect("UTF-8 formula");
    assert!(formula.contains("jackin-capsule/linux-amd64/jackin-capsule"));
    assert!(!formula.contains("jackin-capsule/linux-x64/jackin-capsule"));
}

#[test]
fn canonical_renderer_and_recognizer_keep_ruby_structure_closed() {
    let spec = spec();
    let projection = validate_homebrew_projection(&spec, &plan(PackageReleaseChannel::Preview))
        .expect("projection");
    let release = literals(&spec, &projection);
    let bytes = render_homebrew_formula(&spec, &release).expect("render");
    let formula = std::str::from_utf8(&bytes).expect("UTF-8 formula");
    assert!(formula.contains(r#"Jackin \#{system} \#@instance \#$global app \"quoted\" safely"#));
    assert!(formula.contains(r"\#@instance"));
    assert!(formula.contains(r"\#$global"));
    assert_formula_blocks_balance(formula);
    assert_eq!(formula.matches("bin.install ").count(), 3);
    assert_eq!(formula.matches("url ").count(), 6);
    assert_eq!(formula.matches("sha256 ").count(), 6);
    assert!(formula.contains("on_macos do\n    on_arm do\n"));
    assert!(formula.contains("on_linux do\n    on_intel do\n"));
    assert!(formula.contains("resource \"jackin-capsule-aarch64-unknown-linux-gnu\" do"));
    assert!(formula.contains("if Hardware::CPU.arm?\n      resource(\"jackin-capsule-aarch64-unknown-linux-gnu\").stage do"));
    assert!(formula.contains("elsif Hardware::CPU.intel?\n      resource(\"jackin-capsule-x86_64-unknown-linux-gnu\").stage do"));
    assert!(!formula.contains("on_linux do\n      resource("));
    assert!(formula.contains(
        "assert_predicate libexec/ \"jackin-capsule/linux-arm64/jackin-capsule\", :executable?"
    ));
    assert!(formula.contains("if OS.linux?\n        assert_match version.to_s, shell_output(\"#{libexec}/jackin-capsule/linux-arm64/jackin-capsule --version\", 0)"));
    assert!(formula.contains(" --version\", 0)\n      end\n      header = File.binread(libexec/"));
    assert!(formula.contains("header.byteslice(18, 2).unpack1(\"v\")"));
    assert_eq!(
        parse_generated_formula(&spec, &projection, &bytes).expect("recognize"),
        release
    );

    for mutation in [
        String::from_utf8(bytes.clone())
            .expect("utf8")
            .replace("bin.install \"jackin\"", "system \"cargo\""),
        String::from_utf8(bytes.clone())
            .expect("utf8")
            .replace("sha256 \"", "sha256: \""),
        String::from_utf8(bytes.clone())
            .expect("utf8")
            .replace("    end\n  end", "  end"),
    ] {
        assert!(parse_generated_formula(&spec, &projection, mutation.as_bytes()).is_err());
    }
}
