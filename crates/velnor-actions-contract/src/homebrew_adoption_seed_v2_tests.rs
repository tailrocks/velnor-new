use super::*;

#[test]
fn protected_transition_rejects_generated_path_escape_and_base_shift() {
    let (mut transition, ..) = fixture();
    transition
        .generated_output_paths
        .push("scripts/evil.py".to_owned());
    assert!(transition.validate().is_err());
    let (transition, _, candidate, claude, generated, bytes) = fixture();
    let locator = bytes
        .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
        .expect("locator");
    assert!(
        transition
            .validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&"e".repeat(40), &transition.base_tree),
                head: (&"c".repeat(40), &"d".repeat(40)),
                changes: &changes(&transition, &generated, &claude, locator),
                candidate_files: &candidate,
                claude: (&claude, &claude),
                locator,
                generated_files: &generated,
                candidate_file_bytes: &bytes,
            },)
            .is_err()
    );
}

#[test]
fn protected_transition_requires_v2_manifest_output_and_exact_candidate_bytes() {
    let (mut transition, ..) = fixture();
    transition.template_version = 1;
    assert!(transition.validate().is_err());

    let (transition, _, candidate, claude, mut generated, mut bytes) = fixture();
    let manifest_path = ".velnor/release-manifest.json";
    generated.remove(manifest_path);
    bytes.remove(manifest_path);
    assert!(
        transition
            .validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (&"c".repeat(40), &"d".repeat(40)),
                changes: &changes(
                    &transition,
                    &generated,
                    &claude,
                    bytes
                        .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                        .expect("locator"),
                ),
                candidate_files: &candidate,
                claude: (&claude, &claude),
                locator: bytes
                    .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                    .expect("locator"),
                generated_files: &generated,
                candidate_file_bytes: &bytes,
            })
            .is_err()
    );

    let (transition, _, candidate, claude, generated, mut bytes) = fixture();
    let altered = br#"{"schema":1,"version":"0.1.54"}"#.to_vec();
    bytes.insert(manifest_path.to_owned(), altered);
    assert!(
        transition
            .validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (&"c".repeat(40), &"d".repeat(40)),
                changes: &changes(
                    &transition,
                    &generated,
                    &claude,
                    bytes
                        .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                        .expect("locator"),
                ),
                candidate_files: &candidate,
                claude: (&claude, &claude),
                locator: bytes
                    .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                    .expect("locator"),
                generated_files: &generated,
                candidate_file_bytes: &bytes,
            })
            .is_err()
    );
}
