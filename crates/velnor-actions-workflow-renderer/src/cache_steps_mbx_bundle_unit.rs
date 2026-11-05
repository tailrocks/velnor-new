// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    RootSlug, SliceId, StateUnit, state_snapshot_plan_expr,
};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_SNAPSHOT_ENV, MBX_BUNDLE_UNIT_ENV,
};
use velnor_actions_contract::workflow::state_seal::{
    STATE_MATRIX_ATTESTED, STATE_MATRIX_SLICE, STATE_MATRIX_SNAPSHOT, STATE_SEAL_ATTESTED_ENV,
    state_attested_plan_expr, state_matrix_expr,
};

use super::{DISCOVERY_RECORD, workspace_root};
use crate::RenderError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnitBinding {
    Fixed(StateUnit),
    ProducerMatrix,
}

impl UnitBinding {
    pub(super) fn snapshot_from_plan(&self) -> bool {
        match self {
            Self::Fixed(unit) => unit.is_attested(),
            Self::ProducerMatrix => true,
        }
    }

    pub(super) fn token(&self) -> String {
        match self {
            Self::Fixed(unit) => unit.token(),
            Self::ProducerMatrix => "$unit".to_owned(),
        }
    }

    pub(super) fn unit_lines(&self) -> Vec<String> {
        match self {
            Self::Fixed(_) => Vec::new(),
            Self::ProducerMatrix => vec![
                format!("unit=\"${{{MBX_BUNDLE_UNIT_ENV}-}}\""),
                "case \"$unit\" in slice-?*) ;; *) unit= ;; esac".to_owned(),
                "case \"$unit\" in *[!a-z0-9-]*) unit= ;; esac".to_owned(),
                format!(
                    "if [ -z \"$unit\" ]; then rm -f \"{DISCOVERY_RECORD}\"; printf '%s\\n' 'reason=unit_malformed' 'save=false' 'restore=false' >> \"$GITHUB_OUTPUT\"; printf '%s\\n' 'velnor: state snapshot disabled reason=unit_malformed'; exit 0; fi"
                ),
            ],
        }
    }

    pub(super) fn env(&self) -> BTreeMap<String, String> {
        match self {
            Self::Fixed(unit) => {
                let mut env = BTreeMap::new();
                if unit.is_attested() {
                    env.insert(
                        STATE_SEAL_ATTESTED_ENV.to_owned(),
                        state_attested_plan_expr(unit),
                    );
                    env.insert(
                        MBX_BUNDLE_SNAPSHOT_ENV.to_owned(),
                        state_snapshot_plan_expr(unit),
                    );
                }
                env
            }
            Self::ProducerMatrix => BTreeMap::from([
                (
                    MBX_BUNDLE_UNIT_ENV.to_owned(),
                    format!("slice-{}", state_matrix_expr(STATE_MATRIX_SLICE)),
                ),
                (
                    STATE_SEAL_ATTESTED_ENV.to_owned(),
                    state_matrix_expr(STATE_MATRIX_ATTESTED),
                ),
                (
                    MBX_BUNDLE_SNAPSHOT_ENV.to_owned(),
                    state_matrix_expr(STATE_MATRIX_SNAPSHOT),
                ),
            ]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxTransport {
    InstallOnly,
    SeedRead(SliceId),
    ProducerMatrix,
    SuiteBundle,
}

impl MbxTransport {
    pub(crate) fn unit(
        self,
        working_directory: Option<&str>,
    ) -> Result<Option<UnitBinding>, RenderError> {
        match self {
            Self::InstallOnly => Ok(None),
            Self::SeedRead(slice) => Ok(Some(UnitBinding::Fixed(StateUnit::Slice(slice)))),
            Self::ProducerMatrix => Ok(Some(UnitBinding::ProducerMatrix)),
            Self::SuiteBundle => {
                let root = workspace_root(working_directory)?;
                RootSlug::from_root(&root)
                    .map(|slug| Some(UnitBinding::Fixed(StateUnit::Suite(slug))))
                    .map_err(|error| RenderError::InvalidWorkflow(format!("state_root:{error}")))
            }
        }
    }
}
