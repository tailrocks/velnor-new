// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn catalog_tool_pins(catalog: &velnor_actions_mise::ToolCatalog) -> Vec<String> {
    velnor_actions_workflow_renderer::native_authority::catalog_tool_pins(catalog)
}
