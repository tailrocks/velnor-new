use super::{assert_source_prep_rejected, producer_home_template, source_prep_workflow};

#[test]
fn standalone_source_prep_verifier_rejects_missing_duplicate_and_extra_environment() {
    let template = producer_home_template();
    let canonical = source_prep_workflow(&template, false);
    let no_config = format!(
        "          MISE_NO_CONFIG: {}\n",
        velnor_actions_workflow_renderer::yaml::quote_scalar(&template.env["MISE_NO_CONFIG"])
    );
    let missing_entry = canonical.replace(&no_config, "");
    assert_ne!(missing_entry, canonical);
    assert_source_prep_rejected(&missing_entry, "missing project-config isolation");

    let duplicate_entry = canonical.replace(&no_config, &format!("{no_config}{no_config}"));
    assert_ne!(duplicate_entry, canonical);
    assert_source_prep_rejected(&duplicate_entry, "duplicate environment keys");

    let duplicate_empty_mapping =
        canonical.replace("        shell:", "        env:\n        shell:");
    assert_ne!(duplicate_empty_mapping, canonical);
    assert_source_prep_rejected(
        &duplicate_empty_mapping,
        "duplicate top-level environment mappings",
    );

    let extra_entry = canonical.replace(
        "        env:\n",
        "        env:\n          UNEXPECTED: \"1\"\n",
    );
    assert_ne!(extra_entry, canonical);
    assert_source_prep_rejected(&extra_entry, "additional caller environment");
}
