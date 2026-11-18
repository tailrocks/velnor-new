// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredWorkflow {
    pub name: String,
    pub file: String,
    pub on: Vec<String>,
    pub permissions: Vec<(String, String)>,
    pub condition: Option<String>,
    pub jobs: Vec<DeclaredJob>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredJob {
    pub id: String,
    pub needs: Vec<String>,
    pub environment: Option<String>,
    pub permissions: Vec<(String, String)>,
    pub timeout_minutes: Option<u16>,
    pub condition: Option<String>,
    pub outputs: Vec<(String, String)>,
    pub steps: Vec<DeclaredStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredStep {
    pub id: Option<String>,
    pub name: Option<String>,
    pub condition: Option<String>,
    pub body: DeclaredBody,
    pub with: Vec<(String, String)>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredBody {
    Action(String),
    Helper(String),
}
