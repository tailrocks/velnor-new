use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError {
    Contract(velnor_actions_contract::ContractError),
    InvalidWorkflow(String),
    PolicyRejected { policy: String, problem: String },
    BadVersion(String),
    BadMarker { expected: String, found: String },
    BadActionRef(String),
    BadCommand(String),
    PrivateSubcommand(String),
    UnsafePath(String),
}

impl Display for RenderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(err) => write!(f, "contract: {err}"),
            Self::InvalidWorkflow(problem) => write!(f, "invalid workflow: {problem}"),
            Self::PolicyRejected { policy, problem } => {
                write!(f, "policy {policy} rejected: {problem}")
            }
            Self::BadVersion(version) => write!(f, "bad version: {version}"),
            Self::BadMarker { expected, found } => {
                write!(f, "bad marker: expected {expected:?}, found {found:?}")
            }
            Self::BadActionRef(problem) => write!(f, "bad action ref: {problem}"),
            Self::BadCommand(problem) => write!(f, "bad command: {problem}"),
            Self::PrivateSubcommand(token) => {
                write!(f, "private subcommand token: {token}")
            }
            Self::UnsafePath(problem) => write!(f, "unsafe path: {problem}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(err) => Some(err),
            _ => None,
        }
    }
}
