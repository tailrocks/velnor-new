//! Closed native operation/phase pairs; declaration scripts narrow package phases.

use super::{RequiredNativePhase as Phase, WorkloadKind as Kind};

impl Phase {
    /// Whether the native domain defines this phase for the operation.
    #[must_use]
    pub const fn allowed_for(self, operation: Kind) -> bool {
        match operation {
            Kind::DockerBuild => matches!(self, Self::Build),
            Kind::BunCi | Kind::NodeCi => matches!(
                self,
                Self::Install
                    | Self::Lint
                    | Self::Typecheck
                    | Self::Check
                    | Self::Build
                    | Self::Test
            ),
            Kind::SwiftTest => matches!(self, Self::Build | Self::Test),
            Kind::RubySyntax => matches!(self, Self::Syntax),
            Kind::Shellcheck => matches!(self, Self::Shellcheck),
            Kind::Reuse => matches!(self, Self::Reuse),
            Kind::GradleCheck | Kind::GradleDatabaseCheck => matches!(self, Self::GradleCheck),
            Kind::CargoAudit => matches!(self, Self::Audit),
            Kind::CargoDeny => matches!(self, Self::Deny),
            Kind::Alint => matches!(self, Self::Config | Self::Alint),
            Kind::TuiNoDefaultGraph => matches!(self, Self::Graph),
            Kind::TuiXtaskPolicy => matches!(self, Self::Policy),
            Kind::TuiXtaskDeps => matches!(self, Self::Deps),
            Kind::TuiXtaskPackage => matches!(self, Self::Package),
            Kind::NativeXcodeProjectCi => matches!(
                self,
                Self::NativeFfi
                    | Self::NativeGenerate
                    | Self::NativeXcodeBuild
                    | Self::NativeXcodeTest
            ),
            Kind::NativeSwiftPackageCi => matches!(
                self,
                Self::NativeFfi | Self::NativeSwiftBuild | Self::NativeSwiftTest
            ),
            Kind::HomebrewAudit => matches!(
                self,
                Self::HomebrewTapLocal
                    | Self::HomebrewAudit
                    | Self::HomebrewAuditFormula
                    | Self::HomebrewAuditCask
            ),
            Kind::PackageUpdateFixture => matches!(self, Self::PackageUpdateFixtures),
        }
    }
}
