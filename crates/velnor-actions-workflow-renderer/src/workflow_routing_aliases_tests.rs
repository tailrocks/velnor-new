use super::Expansion;
use crate::{
    RenderError,
    yaml::{AnchorName, Yaml},
};

fn anchor(name: &str) -> AnchorName {
    AnchorName::new(name).expect("valid anchor")
}

fn fails(value: &Yaml, limits: (usize, usize, usize)) -> bool {
    let mut expansion = Expansion {
        max_depth: limits.0,
        max_nodes: limits.1,
        max_bytes: limits.2,
        ..Expansion::default()
    };
    expansion.walk(value, 0).is_err()
}

#[test]
fn rejects_unknown_forward_and_self_referential_aliases() {
    assert!(fails(&Yaml::Alias(anchor("later")), (32, 100, 1024)));
    assert!(fails(
        &Yaml::Map(vec![
            ("before".to_owned(), Yaml::Alias(anchor("later"))),
            (
                "later".to_owned(),
                Yaml::AnchoredScalar {
                    name: anchor("later"),
                    value: "image:v1".to_owned(),
                },
            ),
        ]),
        (32, 100, 1024),
    ));
    assert!(fails(
        &Yaml::AnchoredMap {
            name: anchor("cycle"),
            entries: vec![("self".to_owned(), Yaml::Alias(anchor("cycle")))],
        },
        (32, 100, 1024),
    ));
}

#[test]
fn bounds_depth_nodes_and_anchored_scalar_bytes() {
    let nested = Yaml::Seq(vec![Yaml::Seq(vec![Yaml::Null])]);
    assert!(fails(&nested, (1, 100, 1024)));
    assert!(fails(&nested, (32, 2, 1024)));
    assert!(fails(
        &Yaml::AnchoredScalar {
            name: anchor("large"),
            value: "123456".to_owned(),
        },
        (32, 10, 5),
    ));
}

#[test]
fn expands_scalar_and_mapping_aliases_to_detached_values() -> Result<(), RenderError> {
    let value = Yaml::Map(vec![
        (
            "base".to_owned(),
            Yaml::AnchoredMap {
                name: anchor("mapping"),
                entries: vec![(
                    "image".to_owned(),
                    Yaml::AnchoredScalar {
                        name: anchor("scalar"),
                        value: "image:v1".to_owned(),
                    },
                )],
            },
        ),
        ("copy".to_owned(), Yaml::Alias(anchor("mapping"))),
        ("value".to_owned(), Yaml::Alias(anchor("scalar"))),
    ]);
    let mut expansion = Expansion::default();
    let output = expansion.walk(&value, 0)?;
    let Yaml::Map(fields) = output else {
        panic!("map")
    };
    assert_eq!(fields[0].1, fields[1].1);
    assert_eq!(fields[2].1, Yaml::str("image:v1"));
    Ok(())
}
