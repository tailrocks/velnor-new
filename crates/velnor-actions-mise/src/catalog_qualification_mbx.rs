//! Official MBX audit delegates to the sole source-builder bootstrap authority.
//! Upstream bytes do not grant Velnor-owned `MbxTransport` behavior.

use super::{DistributionHost, DistributionTool, QualifiedDistribution};
use crate::MiseError;

pub(super) fn official(host: DistributionHost) -> Result<QualifiedDistribution, MiseError> {
    super::records::official(DistributionTool::Mbx, host)
}
