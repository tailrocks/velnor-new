use super::*;

#[test]
fn jackin_payload_display_names_describe_all_six_literal_pairs() {
    let mut spec = fixture();
    let mut jackin = spec.policy.products[1].clone();
    jackin.id = "jackin".to_owned();
    jackin.package = "jackin".to_owned();
    jackin.bins = ["jackin", "jackin-role", "jackin-usage-broker"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut capsule = spec.policy.products[0].clone();
    capsule.id = "jackin-capsule".to_owned();
    capsule.package = "jackin-capsule".to_owned();
    capsule.bins = vec!["jackin-capsule".to_owned()];
    capsule.targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    spec.policy.products = vec![jackin, capsule];
    bind_version_owners(&mut spec);
    let jobs = production::jobs(&spec).expect("production");
    assert_eq!(jobs.len(), 2);
    let rendered = render_consumer_preview(&spec).expect("preview");
    assert!(
        rendered
            .bytes
            .contains("name: Build ${{ matrix.product_id }} / ${{ matrix.target }}\n")
    );
    let (_, assembly) = jobs.last().expect("assembly");
    assert_eq!(field(assembly, "name"), &Yaml::str("preview-assemble"));
    assert!(rendered.bytes.contains("name: preview-signer\n"));
}
