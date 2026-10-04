//! Canonical requested installation data; source bytes stay universal.

use super::{DistributionHost, MiseError, PinnedTool, QualifiedDistribution, ToolCatalog};

pub(super) fn encode(
    catalog: &ToolCatalog,
    host: DistributionHost,
    tools: &[PinnedTool],
) -> Result<String, MiseError> {
    let text = text(catalog, host, tools)?;
    Ok(encode_hex(text.as_bytes()))
}

pub(super) fn text(
    catalog: &ToolCatalog,
    host: DistributionHost,
    tools: &[PinnedTool],
) -> Result<String, MiseError> {
    let mut rows = vec!["velnor-tool-prepare-v1".to_owned()];
    for tool in tools {
        let selector = catalog.native_tool_spec(host, *tool)?;
        let binary = match tool {
            PinnedTool::Opentofu => "tofu",
            PinnedTool::Nextest => "cargo-nextest",
            _ => tool.tool_name(),
        };
        if ToolCatalog::requires_native_host(*tool) {
            let record = catalog.native_distribution(host, *tool)?;
            row(&mut rows, &["tool", &selector, binary, record.version()])?;
            append_native(&mut rows, &record)?;
        } else {
            row(
                &mut rows,
                &["tool", &selector, binary, catalog.version(*tool)],
            )?;
        }
    }
    let config = rows.join("\n");
    if config.len() > 32_768 {
        return Err(super::invalid());
    }
    Ok(config)
}

fn row(rows: &mut Vec<String>, fields: &[&str]) -> Result<(), MiseError> {
    if fields
        .iter()
        .any(|field| field.contains(['\n', '\r', '\t', '\0']))
    {
        return Err(super::invalid());
    }
    rows.push(fields.join("\t"));
    Ok(())
}

/// Canonical selected-record configuration shared by closed owner profiles.
pub(super) fn encode_records(records: &[QualifiedDistribution]) -> Result<String, MiseError> {
    let mut rows = vec!["velnor-tool-prepare-v1".to_owned()];
    for record in records {
        let binary = match record.tool() {
            super::DistributionTool::Java => "java",
            super::DistributionTool::Gradle => "gradle",
            _ => return Err(super::invalid()),
        };
        row(
            &mut rows,
            &["tool", record.selector(), binary, record.version()],
        )?;
        append_native(&mut rows, record)?;
    }
    let text = rows.join("\n");
    if text.len() > 32_768 {
        return Err(super::invalid());
    }
    Ok(encode_hex(text.as_bytes()))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

fn append_native(rows: &mut Vec<String>, record: &QualifiedDistribution) -> Result<(), MiseError> {
    let plan = record.required_install_plan()?;
    row(
        rows,
        &[
            "plan",
            record.selector(),
            record.asset_url(),
            record.archive_sha256(),
            &plan.strip_components().to_string(),
            plan.bin_path(),
            plan.root_relative_path(),
        ],
    )?;
    for entry in record.launch_entries() {
        row(
            rows,
            &[
                "launch",
                record.selector(),
                entry.installed_relative_path().ok_or_else(super::invalid)?,
                entry.sha256(),
            ],
        )?;
    }
    for environment in plan.environment() {
        row(
            rows,
            &[
                "environment",
                record.selector(),
                environment.name(),
                environment.relative_path(),
            ],
        )?;
    }
    Ok(())
}
