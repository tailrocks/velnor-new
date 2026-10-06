//! Explicit tool preparation dependencies preserve read-only event execution.
use velnor_actions_contract::Job;

/// Wait for advisory tool preparation without suppressing verified local repair.
pub(super) fn depend_on_producer(consumer: &mut Job, producer: &str) {
    if consumer.needs.iter().any(|need| need == producer) {
        return;
    }
    let prior_success = if consumer.needs.is_empty() {
        "true".to_owned()
    } else {
        consumer
            .needs
            .iter()
            .map(|need| format!("needs['{need}'].result == 'success'"))
            .collect::<Vec<_>>()
            .join(" && ")
    };
    let original = consumer
        .condition
        .take()
        .unwrap_or_else(|| "success()".to_owned());
    let original = original
        .strip_prefix("${{")
        .and_then(|value| value.strip_suffix("}}"))
        .map_or(original.as_str(), str::trim);
    let original = original.replace("success()", &format!("({prior_success})"));
    consumer.needs.push(producer.to_owned());
    consumer.condition = Some(format!("!cancelled() && ({original})"));
}

#[cfg(test)]
#[path = "workflow_tool_dependencies_tests.rs"]
mod tests;
