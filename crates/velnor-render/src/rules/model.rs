// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Expr(String),
    BoundedExpr { text: String, max_bytes: usize },
    Int(u64),
    Bool(bool),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedStep {
    pub id: Option<String>,
    pub name: String,
    pub condition: Option<String>,
    pub continue_on_error: bool,
    pub uses: Option<String>,
    pub uses_comment: Option<String>,
    pub with: Vec<(String, Value)>,
    pub working_directory: Option<String>,
    pub env: Vec<(String, Value)>,
    pub shell: Option<String>,
    pub run: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedConcurrency {
    pub group: String,
    pub cancel_in_progress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderedMatrixSource {
    /// `fromJSON(<expression>)`, such as `needs.plan.outputs.matrix`.
    Output(String),
    /// Static axes, each with its values.
    Axes {
        axes: Vec<(String, Vec<String>)>,
        include: Vec<Vec<(String, String)>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMatrix {
    pub source: RenderedMatrixSource,
    pub fail_fast: Option<bool>,
    pub max_parallel: Option<u16>,
}

/// A job `container:` or one service; `ports` is empty for a job container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedContainer {
    pub image: String,
    pub ports: Vec<String>,
    pub options: Option<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedJob {
    pub id: String,
    pub name: Option<String>,
    pub check_name: Option<String>,
    pub comment: Option<String>,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub environment: Option<String>,
    pub environment_note: Option<String>,
    pub permissions: Vec<(String, String)>,
    pub no_permissions: bool,
    pub concurrency: Option<RenderedConcurrency>,
    pub outputs: Vec<(String, String)>,
    pub matrix: Option<RenderedMatrix>,
    pub container: Option<RenderedContainer>,
    pub services: Vec<(String, RenderedContainer)>,
    /// A job that calls a reusable workflow has `uses` and `with` and no runner or steps.
    pub uses: Option<String>,
    pub with: Vec<(String, Value)>,
    pub runs_on: String,
    /// When set, `runs-on` is `matrix.<axis>` and `runs_on` is unused.
    pub runs_on_axis: Option<String>,
    pub fork_runner: Option<String>,
    pub timeout_minutes: u16,
    pub env: Vec<(String, Value)>,
    pub cache_mode: Option<String>,
    pub steps: Vec<RenderedStep>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedWorkflow {
    pub jobs: Vec<RenderedJob>,
}
