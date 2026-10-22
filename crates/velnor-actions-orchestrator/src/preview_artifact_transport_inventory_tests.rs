use super::*;

#[test]
fn inventory_requires_stable_totals_full_pages_unique_positive_ids_and_count_bound() {
    let full = (1..=100)
        .map(|id| ArtifactRecord { id, ..record() })
        .collect::<Vec<_>>();
    let valid = vec![
        ArtifactPage {
            total_count: 101,
            artifacts: full.clone(),
        },
        ArtifactPage {
            total_count: 101,
            artifacts: vec![ArtifactRecord {
                id: 101,
                ..record()
            }],
        },
    ];
    let api = Fake::new(vec![]);
    *api.pages.borrow_mut() = valid.clone().into();
    assert_eq!(
        inventory(&api, 202, deadline()).expect("all pages").len(),
        101
    );
    let mut cases = vec![
        vec![ArtifactPage {
            total_count: 2,
            artifacts: vec![record()],
        }],
        vec![ArtifactPage {
            total_count: 1,
            artifacts: vec![],
        }],
        vec![ArtifactPage {
            total_count: 0,
            artifacts: vec![record()],
        }],
        vec![ArtifactPage {
            total_count: 1001,
            artifacts: full,
        }],
        vec![ArtifactPage {
            total_count: 2,
            artifacts: vec![record(), record()],
        }],
        vec![ArtifactPage {
            total_count: 1,
            artifacts: vec![ArtifactRecord { id: 0, ..record() }],
        }],
    ];
    let mut changed = valid.clone();
    changed[1].total_count = 102;
    cases.push(changed);
    let mut short = valid.clone();
    short[1].artifacts.clear();
    cases.push(short);
    let mut duplicate = valid;
    duplicate[1].artifacts[0].id = 1;
    cases.push(duplicate);
    for pages in cases {
        let api = Fake::new(vec![]);
        *api.pages.borrow_mut() = pages.into();
        assert!(inventory(&api, 202, deadline()).is_err());
        assert!(api.downloads.borrow().is_empty());
    }
}
