use crate::impl_common::TestResult;

pub(super) fn assert_direct_resolver_step(yaml: &str) -> TestResult {
    let lines = yaml.lines().collect::<Vec<_>>();
    let index = lines
        .iter()
        .position(|line| line.trim_start() == "- name: Resolve project tool versions")
        .ok_or("resolver is not a direct workflow/composite step")?;
    let name_indent = lines[index].len() - lines[index].trim_start().len();
    let steps_indent = lines[..index]
        .iter()
        .rev()
        .find_map(|line| {
            let trimmed = line.trim_start();
            (trimmed == "steps:" && line.len() - trimmed.len() < name_indent)
                .then_some(line.len() - trimmed.len())
        })
        .ok_or("resolver is not inside a steps sequence")?;
    assert_eq!(
        name_indent,
        steps_indent + 2,
        "resolver name must be a direct steps item: {yaml}"
    );
    let fields = lines[index + 1..]
        .iter()
        .take_while(|line| {
            line.trim().is_empty() || line.len() - line.trim_start().len() > name_indent
        })
        .filter_map(|line| {
            let trimmed = line.trim_start();
            (line.len() - trimmed.len() == name_indent + 2).then_some(trimmed)
        })
        .collect::<Vec<_>>();
    for field in ["env:", "shell: ", "run: "] {
        assert!(
            fields.iter().any(|line| line.starts_with(field)),
            "resolver {field:?} must be a direct field at indent {}: {yaml}",
            name_indent + 2
        );
    }
    Ok(())
}
