use super::super::wire::DoctorProbeReply;
use crate::readiness::Readiness;

pub(super) fn completed_result(
    reply: DoctorProbeReply,
    readiness: Option<Readiness>,
) -> Option<serde_json::Value> {
    if reply.state != "completed" {
        return None;
    }
    let report = reply.report?;
    let mut result = serde_json::json!({
        "request_id": reply.request_id,
        "state": reply.state,
        "report": report,
    });
    if let Some(readiness) = readiness {
        result["readiness"] = serde_json::json!(readiness);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::completed_result;
    use crate::controller_ipc::wire::DoctorProbeReply;

    #[test]
    fn completed_doctor_result_keeps_unready_state_beside_probe_facts() {
        let reply = DoctorProbeReply {
            request_id: Some(17),
            state: "completed".to_owned(),
            report: Some(serde_json::json!({
                "cpu_count": 8,
                "docker_root_fs_type_name": "btrfs",
            })),
        };
        let result = completed_result(reply, Some(crate::readiness::Readiness::Reconciling))
            .expect("completed report has facts");

        assert_eq!(result["request_id"], 17);
        assert_eq!(result["state"], "completed");
        assert_eq!(result["readiness"], "reconciling");
        assert_eq!(result["report"]["cpu_count"], 8);
        assert_eq!(result["report"]["docker_root_fs_type_name"], "btrfs");
    }

    #[test]
    fn completed_result_requires_a_report() {
        let reply = DoctorProbeReply {
            request_id: Some(17),
            state: "completed".to_owned(),
            report: None,
        };
        assert!(completed_result(reply, Some(crate::readiness::Readiness::Reconciling)).is_none());
    }

    #[test]
    fn old_server_result_keeps_the_previous_cli_shape() {
        let reply = DoctorProbeReply {
            request_id: Some(17),
            state: "completed".to_owned(),
            report: Some(serde_json::json!({"cpu_count": 8})),
        };
        let result = completed_result(reply, None).expect("completed report has facts");
        assert_eq!(
            result,
            serde_json::json!({
                "request_id": 17,
                "state": "completed",
                "report": {"cpu_count": 8},
            })
        );
    }
}
