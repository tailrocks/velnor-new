// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::{RepoView, RequiredFeature};

use crate::facts::TestResult;
use crate::image_workflow::edited;

const IMAGE: &str = "features = [\"tls\"]\n";

fn gated(
    features: &str,
    required: &[RequiredFeature],
    recorded: bool,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = edited("image-workspace", |text| text.replacen(IMAGE, features, 1))?;
    for metadata in view.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            for target in &mut member.targets {
                if target.name.as_str() == "legacy-server" {
                    target.requires_features = true;
                    target.required_features = if recorded {
                        required.to_vec()
                    } else {
                        Vec::new()
                    };
                }
            }
        }
    }
    Ok(view)
}

fn tls(enabled_by: &[&str]) -> Vec<RequiredFeature> {
    vec![RequiredFeature {
        name: "tls".to_owned(),
        enabled_by: enabled_by.iter().map(|name| (*name).to_owned()).collect(),
    }]
}

fn accepted(view: &RepoView) -> bool {
    expected_files(view).is_ok()
}

#[test]
fn the_features_must_name_every_feature_a_binary_requires() -> TestResult {
    assert!(accepted(&gated("features = [\"tls\"]\n", &tls(&[]), true)?));
    assert!(accepted(&gated(
        "features = [\"legacy-server/tls\"]\n",
        &tls(&[]),
        true
    )?));
    for features in [
        "features = [\"metrics\"]\n",
        "features = []\n",
        "default_features = false\nfeatures = [\"metrics\"]\n",
    ] {
        let view = gated(features, &tls(&[]), true)?;
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Image(ImageError::BinNeedsFeatures { ref feature, .. })) if feature == "tls"
            ),
            "{features}"
        );
    }
    Ok(())
}

#[test]
fn a_feature_that_enables_the_required_one_or_a_default_feature_counts() -> TestResult {
    assert!(accepted(&gated(
        "features = [\"full\"]\n",
        &tls(&["full"]),
        true
    )?));
    assert!(accepted(&gated(
        "features = []\n",
        &tls(&["default"]),
        true
    )?));
    assert!(accepted(&gated(
        "default_features = false\nfeatures = [\"default\"]\n",
        &tls(&["default"]),
        true
    )?));
    assert!(!accepted(&gated(
        "default_features = false\nfeatures = []\n",
        &tls(&["default"]),
        true
    )?));
    Ok(())
}

#[test]
fn required_features_the_recorded_metadata_does_not_name_are_refused_with_a_regenerate_hint()
-> TestResult {
    let view = gated("features = [\"tls\"]\n", &tls(&[]), false)?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(
            ImageError::BinFeaturesUnrecorded { .. }
        ))
    ));
    Ok(())
}
