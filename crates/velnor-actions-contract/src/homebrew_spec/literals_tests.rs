use super::super::tests::*;

#[test]
fn literal_constructor_rejects_bad_identity_and_zero_digest() {
    let spec = spec();
    let projection = validate_homebrew_projection(&spec, &plan(PackageReleaseChannel::Preview))
        .expect("projection");
    let mut hashes = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "b".repeat(64)))
        .collect::<BTreeMap<_, _>>();
    hashes.insert("attacker.tar.gz".to_owned(), "c".repeat(64));
    assert!(
        HomebrewFormulaReleaseLiterals::new(
            &spec,
            &projection,
            SOURCE_SHA.to_owned(),
            "1.2.3-preview.4+0123456".to_owned(),
            format!("preview-{SOURCE_SHA}"),
            hashes
        )
        .is_err()
    );

    for version in [
        "1.2.3-preview.0+0123456",
        "1.2.3-preview.04+0123456",
        "1.2.3-preview.4+7654321",
        "9.9.9-preview.4+0123456",
        "1.2.3",
    ] {
        let hashes = projection
            .selected_payloads()
            .iter()
            .map(|payload| (payload.asset_name().to_owned(), "b".repeat(64)))
            .collect();
        assert!(
            HomebrewFormulaReleaseLiterals::new(
                &spec,
                &projection,
                SOURCE_SHA.to_owned(),
                version.to_owned(),
                format!("preview-{SOURCE_SHA}"),
                hashes
            )
            .is_err(),
            "{version}"
        );
    }
    let hashes = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "0".repeat(64)))
        .collect();
    assert!(
        HomebrewFormulaReleaseLiterals::new(
            &spec,
            &projection,
            SOURCE_SHA.to_owned(),
            "1.2.3-preview.4+0123456".to_owned(),
            format!("preview-{SOURCE_SHA}"),
            hashes
        )
        .is_err()
    );
    let hashes = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "b".repeat(64)))
        .collect();
    assert!(
        HomebrewFormulaReleaseLiterals::new(
            &spec,
            &projection,
            "0".repeat(40),
            "1.2.3-preview.4+0000000".to_owned(),
            format!("preview-{}", "0".repeat(40)),
            hashes,
        )
        .is_err()
    );
}
