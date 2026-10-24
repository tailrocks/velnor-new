use super::{
    BTreeSet, ContractError, HomebrewFormulaSpec, HomebrewPayloadRole, HomebrewSelectedPayload,
    PackageReleaseChannel, PackageReleasePayloadFormat, PackageReleasePlan,
    PackageReleasePlanProduct, ValidatedHomebrewProjection, invalid,
};

impl ValidatedHomebrewProjection {
    /// Authenticated release channel selected from the package plan.
    #[must_use]
    pub const fn channel(&self) -> PackageReleaseChannel {
        self.channel
    }
    /// Source repository slug from the package plan.
    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }
    /// Selected payloads, derived from exact plan entries.
    #[must_use]
    pub fn selected_payloads(&self) -> &[HomebrewSelectedPayload] {
        &self.selected
    }
}

impl HomebrewSelectedPayload {
    /// Plan product ID.
    #[must_use]
    pub fn product(&self) -> &str {
        &self.product
    }
    /// Exact selected Rust target.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }
    /// Exact immutable payload asset name.
    #[must_use]
    pub fn asset_name(&self) -> &str {
        &self.asset_name
    }
    /// Exact archive members.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }
    /// Selected role.
    #[must_use]
    pub const fn role(&self) -> HomebrewPayloadRole {
        self.role
    }
}

/// Resolve exact selected host and auxiliary payloads against the signed release plan.
///
/// # Errors
/// Returns an error when declared products, targets, members, or archive formats do not match the plan.
pub fn validate_homebrew_projection(
    spec: &HomebrewFormulaSpec,
    plan: &PackageReleasePlan,
) -> Result<ValidatedHomebrewProjection, ContractError> {
    spec.validate()?;
    plan.validate()?;
    if !spec
        .channels
        .iter()
        .any(|item| item.channel == plan.channel)
    {
        return Err(invalid("formula_channel_not_declared"));
    }
    let mut selected = Vec::new();
    let mut products = BTreeSet::new();
    products.insert(spec.host.product.as_str());
    if spec
        .auxiliaries
        .iter()
        .any(|item| !products.insert(item.product.as_str()))
    {
        return Err(invalid("selected_product_collision"));
    }
    let host = find_product(plan, &spec.host.product)?;
    if host.bins != spec.host.bins || !is_subset(&spec.host.targets, &host.targets) {
        return Err(invalid("host_plan_projection_mismatch"));
    }
    append_product_payloads(
        &mut selected,
        host,
        &spec.host.targets,
        HomebrewPayloadRole::Host,
    )?;
    for auxiliary in &spec.auxiliaries {
        let product = find_product(plan, &auxiliary.product)?;
        if !is_subset(&auxiliary.targets, &product.targets)
            || auxiliary
                .files
                .iter()
                .any(|file| !product.bins.contains(&file.member))
            || auxiliary.targets.iter().any(|target| {
                auxiliary
                    .files
                    .iter()
                    .filter(|file| &file.target == target)
                    .map(|file| file.member.as_str())
                    .collect::<BTreeSet<_>>()
                    != product
                        .bins
                        .iter()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>()
            })
        {
            return Err(invalid("auxiliary_plan_projection_mismatch"));
        }
        append_product_payloads(
            &mut selected,
            product,
            &auxiliary.targets,
            HomebrewPayloadRole::Auxiliary,
        )?;
    }
    let mut asset_names = BTreeSet::new();
    if selected
        .iter()
        .any(|payload| !asset_names.insert(payload.asset_name.as_str()))
    {
        return Err(invalid("selected_payload_collision"));
    }
    Ok(ValidatedHomebrewProjection {
        channel: plan.channel,
        repository: plan.repository.clone(),
        base_version: plan.base_version.clone(),
        selected,
    })
}

pub(super) fn append_product_payloads(
    out: &mut Vec<HomebrewSelectedPayload>,
    product: &PackageReleasePlanProduct,
    targets: &[String],
    role: HomebrewPayloadRole,
) -> Result<(), ContractError> {
    if product.format != PackageReleasePayloadFormat::TarGz {
        return Err(invalid("unsupported_selected_payload"));
    }
    for target in targets {
        if product.targets.binary_search(target).is_err() {
            return Err(invalid("selected_target_missing"));
        }
        out.push(HomebrewSelectedPayload {
            product: product.id.clone(),
            target: target.clone(),
            asset_name: format!("{}-{target}.tar.gz", product.id),
            members: product.bins.clone(),
            role,
        });
    }
    Ok(())
}

pub(super) fn find_product<'a>(
    plan: &'a PackageReleasePlan,
    id: &str,
) -> Result<&'a PackageReleasePlanProduct, ContractError> {
    let mut matches = plan.products.iter().filter(|product| product.id == id);
    let product = matches
        .next()
        .ok_or_else(|| invalid("selected_product_missing"))?;
    if matches.next().is_some() {
        return Err(invalid("selected_product_ambiguous"));
    }
    Ok(product)
}

pub(super) fn is_subset(selected: &[String], available: &[String]) -> bool {
    selected
        .iter()
        .all(|value| available.binary_search(value).is_ok())
}

pub(super) fn validate_projection_spec(
    spec: &HomebrewFormulaSpec,
    projection: &ValidatedHomebrewProjection,
) -> Result<(), ContractError> {
    if !spec
        .channels
        .iter()
        .any(|channel| channel.channel == projection.channel)
        || !crate::vcs::is_safe_github_repository(&projection.repository)
    {
        return Err(invalid("projection_channel_or_repository_mismatch"));
    }
    let mut expected = Vec::new();
    append_spec_payloads(
        &mut expected,
        &spec.host.product,
        &spec.host.targets,
        &spec.host.bins,
        HomebrewPayloadRole::Host,
    );
    for auxiliary in &spec.auxiliaries {
        for target in &auxiliary.targets {
            let members = auxiliary
                .files
                .iter()
                .filter(|file| &file.target == target)
                .map(|file| file.member.clone())
                .collect::<Vec<_>>();
            append_spec_payloads(
                &mut expected,
                &auxiliary.product,
                std::slice::from_ref(target),
                &members,
                HomebrewPayloadRole::Auxiliary,
            );
        }
    }
    if expected != projection.selected {
        return Err(invalid("projection_spec_selection_mismatch"));
    }
    Ok(())
}

pub(super) fn append_spec_payloads(
    out: &mut Vec<HomebrewSelectedPayload>,
    product: &str,
    targets: &[String],
    members: &[String],
    role: HomebrewPayloadRole,
) {
    out.extend(targets.iter().map(|target| HomebrewSelectedPayload {
        product: product.to_owned(),
        target: target.clone(),
        asset_name: format!("{product}-{target}.tar.gz"),
        members: members.to_vec(),
        role,
    }));
}
