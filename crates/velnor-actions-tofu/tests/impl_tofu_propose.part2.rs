/// Independent mutation of any root authority fails before adapter admission.
#[test]
fn proposal_root_authorities_must_all_agree_merged() {
    for root in ["", "root", "infra/日本語"] {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let task = propose_task(&group(root, kind)).expect("proposal");
            assert_eq!(
                velnor_actions_tofu::normalized_root_for_proposal(&task).expect("admit"),
                root
            );
            let mut mutations = Vec::new();
            let mut empty_alias = task.clone();
            empty_alias.identity.project_root.clear();
            mutations.push(empty_alias);
            let mut changed = task.clone();
            changed.identity.project_root = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_key = key_for_root("different");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_id = key_for_root("different");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_path = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.component_id = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.task_id = task_id_for_root("different", kind, "default").expect("ID");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.payload = tofu_payload_argv(kind, "different").expect("argv");
            mutations.push(changed);
            for changed in mutations {
                assert!(velnor_actions_tofu::normalized_root_for_proposal(&changed).is_err());
            }
        }
    }
}
