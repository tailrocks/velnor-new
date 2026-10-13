impl ToolCatalog {
    /// Qualified pins; trusted identities require the separate validation gate.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            rust: RUST_VERSION.to_owned(),
            mr_boxington: MR_BOXINGTON_VERSION.to_owned(),
            gh: GH_VERSION.to_owned(),
            actionlint: ACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            zizmor: ZIZMOR_VERSION.to_owned(),
            nextest: NEXTEST_VERSION.to_owned(),
            opentofu: OPENTOFU_VERSION.to_owned(),
        }
    }

    /// Explicit versions; each of the eight configurable slots must be exact.
    ///
    /// Only version exactness is enforced here: the catalog owns pins,
    /// not artifact trust. Trusted identity validation is the separate
    /// [`Self::validate_identities`] gate (P03-8b).
    ///
    /// Only version exactness is enforced here: the catalog owns pins,
    /// not artifact trust. Trusted identity validation is the separate
    /// [`Self::validate_identities`] gate (P03-8b).
    ///
    /// # Errors
    /// Returns [`MiseError::InvalidToolVersion`] for non-exact versions.
    #[expect(
        clippy::too_many_arguments,
        reason = "catalog carries eight exact pins at once"
    )]
    pub fn new(
        rust: &str,
        mr_boxington: &str,
        gh: &str,
        actionlint: &str,
        shellcheck: &str,
        zizmor: &str,
        nextest: &str,
        opentofu: &str,
    ) -> Result<Self, MiseError> {
        validate_exact_version(PinnedTool::Rust.tool_name(), rust)?;
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), mr_boxington)?;
        validate_exact_version(PinnedTool::Gh.tool_name(), gh)?;
        validate_exact_version(PinnedTool::Actionlint.tool_name(), actionlint)?;
        validate_exact_version(PinnedTool::Shellcheck.tool_name(), shellcheck)?;
        validate_exact_version(PinnedTool::Zizmor.tool_name(), zizmor)?;
        validate_exact_version(PinnedTool::Nextest.tool_name(), nextest)?;
        validate_exact_version(PinnedTool::Opentofu.tool_name(), opentofu)?;
        Ok(Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            actionlint: actionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            zizmor: zizmor.to_owned(),
            nextest: nextest.to_owned(),
            opentofu: opentofu.to_owned(),
        })
    }

    /// Pinned identity record for one tool (ver §2).
    ///
    /// The digest is the qualified artifact SHA-256 once freshness
    /// binds it (`OpenTofu` is bound since T15); every other slot
    /// keeps the explicitly unqualified placeholder, so its record
    /// never validates as trusted and no trust decision may consume
    /// it (P03-8b).
    #[must_use]
    pub fn tool_identity(&self, tool: PinnedTool) -> ToolIdentity {
        ToolIdentity {
            name: tool.tool_name().to_owned(),
            version: self.version(tool).to_owned(),
            source: tool_source(tool, self.version(tool)),
            platforms: TOOL_PLATFORMS.iter().map(ToString::to_string).collect(),
            digest: self.tool_digest(tool).to_owned(),
        }
    }

    /// Qualified artifact digest for one tool, or the placeholder.
    ///
    /// Qualified `OpenTofu` and Python pins carry artifact digests. Other
    /// versions keep the placeholder so unqualified pins never validate.
    fn tool_digest(&self, tool: PinnedTool) -> &str {
        match tool {
            PinnedTool::Opentofu if self.version(tool) == OPENTOFU_VERSION => {
                OPENTOFU_SHA256_LINUX_AMD64
            }
            PinnedTool::Python if self.version(tool) == PYTHON_VERSION => PYTHON_PBS_SHA256,
            _ => PLACEHOLDER_DIGEST,
        }
    }

    /// Trusted gate: refuses every identity whose artifact digest remains a placeholder.
    ///
    /// # Errors
    /// Returns [`MiseError::Contract`] for the first invalid identity.
    pub fn validate_identities(&self) -> Result<(), MiseError> {
        for tool in PinnedTool::ALL {
            self.tool_identity(tool)
                .validate("catalog")
                .map_err(|err| MiseError::Contract {
                    problem: err.to_string(),
                })?;
        }
        Ok(())
    }

    /// Exact pinned version for one tool.
    #[must_use]
    pub fn version(&self, tool: PinnedTool) -> &str {
        match tool {
            PinnedTool::Rust => &self.rust,
            PinnedTool::MrBoxington => &self.mr_boxington,
            PinnedTool::Gh => &self.gh,
            PinnedTool::Actionlint => &self.actionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Zizmor => &self.zizmor,
            PinnedTool::Nextest => &self.nextest,
            PinnedTool::Opentofu => &self.opentofu,
            PinnedTool::ReleasePlz => RELEASE_PLZ_VERSION,
            PinnedTool::Reuse => REUSE_VERSION,
            PinnedTool::Python => PYTHON_VERSION,
            PinnedTool::Uv => UV_VERSION,
        }
    }

    /// Mise selector for one tool, with backend-specific selectors for
    /// Nextest, Python, and REUSE.
    ///
    /// Rust carries no inline options because the core backend ignores its
    /// component options in config-less specs; `PrepareRustComponents` adds
    /// them separately. Python's HTTP selector binds its artifact and layout.
    #[must_use]
    pub fn tool_spec(&self, tool: PinnedTool) -> String {
        match tool {
            PinnedTool::Nextest => {
                format!("{NEXTEST_TOOL_SPEC_PREFIX}@{}", self.version(tool))
            }
            PinnedTool::Reuse => format!(
                "pipx:reuse[extras=charset-normalizer,uvx_args=\"--python {PYTHON_VERSION} --no-python-downloads\"]@{}",
                self.version(tool)
            ),
            PinnedTool::Python => python_tool_spec(self.version(tool)),
            _ => format!("{}@{}", tool.tool_name(), self.version(tool)),
        }
    }

    /// Exact `rustup` toolchain name (`<rust-exact>-<target-triple>`) that
    /// `mise install rust@<exact>` creates; never an ambient toolchain.
    #[must_use]
    pub fn rust_toolchain_name(&self) -> String {
        self.rust_toolchain_name_for_host(RUST_TARGET_TRIPLE)
    }

    /// Exact `rustup` toolchain name on one build host triple.
    ///
    /// Cross-compile legs address the host toolchain (where `mise install`
    /// ran), never the target triple being built for.
    #[must_use]
    pub fn rust_toolchain_name_for_host(&self, host: &str) -> String {
        format!("{}-{host}", self.version(PinnedTool::Rust))
    }

    /// Mise selectors for several tools, in the given order.
    #[must_use]
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Vec<String> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }

    /// Exact `RUSTUP_TOOLCHAIN` value: the pinned Rust version.
    ///
    /// Rendered Cargo steps set this override so an inspected project
    /// toolchain file cannot select another compiler.
    #[must_use]
    pub fn rustup_toolchain(&self) -> String {
        self.version(PinnedTool::Rust).to_owned()
    }

    /// Fail unless the action-installed MBX equals the catalog pin (action SHA alone never proves it).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for a loose or mismatched version.
    pub fn reconcile_action_mbx(&self, reported: &str) -> Result<(), MiseError> {
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), reported)?;
        (reported == self.version(PinnedTool::MrBoxington))
            .then_some(())
            .ok_or_else(|| invalid_version(PinnedTool::MrBoxington.tool_name(), reported))
    }
}
