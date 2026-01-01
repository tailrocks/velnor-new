use super::super::projection::{ProbeProjection, VerifiedProbeImage, test_projection_digest};

#[test]
fn rebuilt_projection_has_stable_digest_and_semantic_changes_change_it() -> Result<(), String> {
    let build = || {
        ProbeProjection::build(
            "d".repeat(32),
            "e".repeat(32),
            "selected-engine".to_owned(),
            "/var/lib/docker",
            VerifiedProbeImage::test_fixture(),
        )
    };
    let first = build().map_err(|error| error.to_string())?;
    let second = build().map_err(|error| error.to_string())?;
    assert_eq!(first.projection_digest, second.projection_digest);

    let mut reordered = first.config.clone();
    let labels = first
        .config
        .labels
        .as_ref()
        .ok_or_else(|| "projection labels are missing".to_owned())?;
    let mut reversed = std::collections::HashMap::new();
    let mut entries: Vec<_> = labels.iter().collect();
    entries.sort_unstable_by_key(|(key, _)| *key);
    entries.reverse();
    for (key, value) in entries {
        reversed.insert(key.clone(), value.clone());
    }
    reordered.labels = Some(reversed);
    assert_eq!(
        first.projection_digest,
        test_projection_digest(&first.options, &reordered).map_err(|error| error.to_string())?
    );

    let mut changed = first.config.clone();
    let Some(labels) = changed.labels.as_mut() else {
        return Err("projection labels are missing".to_owned());
    };
    labels.insert("velnor.instance".to_owned(), "f".repeat(32));
    let changed_digest =
        test_projection_digest(&first.options, &changed).map_err(|error| error.to_string())?;
    assert_ne!(first.projection_digest, changed_digest);
    Ok(())
}
