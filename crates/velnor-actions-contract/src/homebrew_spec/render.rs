// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ContractError, HomebrewAuxiliaryTest, HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec,
    HomebrewInstalledMode, HomebrewPayloadRole, MAX_FORMULA_BYTES, PackageReleaseChannel, invalid,
    is_sha256, ruby_assignment, ruby_cpu_predicate, ruby_platform_open, ruby_quote,
};

pub fn render_homebrew_formula(
    spec: &HomebrewFormulaSpec,
    literals: &HomebrewFormulaReleaseLiterals,
) -> Result<Vec<u8>, ContractError> {
    spec.validate()?;
    if spec.digest()? != literals.spec_digest
        || !spec
            .channels
            .iter()
            .any(|channel| channel.channel == literals.channel)
        || literals.assets.len() != literals.selected.len()
        || literals.selected.iter().any(|payload| {
            !literals
                .assets
                .iter()
                .any(|asset| asset.name == payload.asset_name)
        })
    {
        return Err(invalid("formula_literals_spec_projection_mismatch"));
    }
    let formula_policy = spec
        .channels
        .iter()
        .find(|channel| channel.channel == literals.channel)
        .ok_or_else(|| invalid("formula_channel_not_declared"))?;
    let mut output = String::with_capacity(4096);
    if let Some(source_sha) = &literals.source_sha {
        output.push_str("# source-sha: ");
        output.push_str(source_sha);
        output.push('\n');
    }
    output.push_str("class ");
    output.push_str(&formula_policy.formula_class);
    output.push_str(" < Formula\n");
    ruby_assignment(&mut output, "desc", &spec.description);
    ruby_assignment(&mut output, "homepage", &spec.homepage);
    output.push_str("  license ");
    output.push_str(&ruby_quote(&spec.license));
    output.push('\n');
    output.push_str("  version ");
    output.push_str(&ruby_quote(&literals.version));
    output.push('\n');
    validate_assets(literals)?;
    if literals.channel == PackageReleaseChannel::Stable {
        output.push_str("  if version.to_s == \"0.0.0\"\n");
        output.push_str("    disable! date: ");
        output.push_str(&ruby_quote(&spec.stable_sentinel.date));
        output.push_str(", because: ");
        output.push_str(&ruby_quote(&spec.stable_sentinel.reason));
        output.push_str("\n  end\n");
    }
    for conflict in &formula_policy.conflicts {
        output.push_str("  conflicts_with ");
        output.push_str(&ruby_quote(conflict));
        output.push('\n');
    }
    render_resources(&mut output, literals)?;
    render_install(&mut output, spec)?;
    render_tests(&mut output, spec)?;
    output.push_str("  end\nend\n");
    if output.len() > MAX_FORMULA_BYTES {
        return Err(invalid("rendered_formula_too_large"));
    }
    Ok(output.into_bytes())
}

fn validate_assets(literals: &HomebrewFormulaReleaseLiterals) -> Result<(), ContractError> {
    if literals.assets.is_empty() || literals.source_sha.is_none() || literals.tag.is_none() {
        return Err(invalid("release_assets_empty"));
    }
    if literals.assets.len() != literals.selected.len()
        || literals.selected.iter().any(|payload| {
            literals
                .assets
                .iter()
                .filter(|asset| asset.name == payload.asset_name)
                .count()
                != 1
        })
        || literals.assets.iter().any(|asset| {
            !is_sha256(&asset.sha256)
                || asset.url
                    != format!(
                        "https://github.com/{}/releases/download/{}/{}",
                        literals.repository,
                        literals.tag.as_deref().unwrap_or_default(),
                        asset.name
                    )
        })
    {
        return Err(invalid("formula_asset_literal_inventory_mismatch"));
    }
    if literals.disabled_stable_sentinel
        && (literals.channel != PackageReleaseChannel::Stable
            || literals.version != "0.0.0"
            || literals.source_sha.as_deref() != Some("0000000000000000000000000000000000000000")
            || literals.tag.as_deref() != Some("v0.0.0")
            || literals.assets.iter().any(|asset| {
                asset.sha256 != "0000000000000000000000000000000000000000000000000000000000000000"
            }))
    {
        return Err(invalid("malformed_stable_sentinel_state"));
    }
    Ok(())
}

fn render_resources(
    output: &mut String,
    literals: &HomebrewFormulaReleaseLiterals,
) -> Result<(), ContractError> {
    for payload in &literals.selected {
        let asset = literals
            .assets
            .iter()
            .find(|asset| asset.name == payload.asset_name)
            .ok_or_else(|| invalid("render_payload_missing"))?;
        if payload.role == HomebrewPayloadRole::Host {
            output.push_str(ruby_platform_open(&payload.target)?);
            output.push_str("      url ");
            output.push_str(&ruby_quote(&asset.url));
            output.push_str("\n      sha256 ");
            output.push_str(&ruby_quote(&asset.sha256));
            output.push_str("\n    end\n  end\n");
        } else {
            let resource = format!("{}-{}", payload.product, payload.target);
            output.push_str("    resource ");
            output.push_str(&ruby_quote(&resource));
            output.push_str(" do\n");
            output.push_str("      url ");
            output.push_str(&ruby_quote(&asset.url));
            output.push_str("\n      sha256 ");
            output.push_str(&ruby_quote(&asset.sha256));
            output.push_str("\n    end\n");
        }
    }
    Ok(())
}

fn render_install(output: &mut String, spec: &HomebrewFormulaSpec) -> Result<(), ContractError> {
    output.push_str("\n  def install\n");
    for bin in &spec.host.bins {
        output.push_str("    bin.install ");
        output.push_str(&ruby_quote(bin));
        output.push('\n');
    }
    for auxiliary in &spec.auxiliaries {
        for (index, target) in auxiliary.targets.iter().enumerate() {
            output.push_str(if index == 0 { "    if " } else { "    elsif " });
            output.push_str(ruby_cpu_predicate(target)?);
            output.push('\n');
            let resource = format!("{}-{target}", auxiliary.product);
            output.push_str("      resource(");
            output.push_str(&ruby_quote(&resource));
            output.push_str(").stage do\n");
            for file in &auxiliary.files {
                if file.target != *target {
                    continue;
                }
                let parent = file
                    .destination
                    .rsplit_once('/')
                    .map_or(".", |(parent, _)| parent);
                output.push_str("        (libexec/");
                output.push_str(&ruby_quote(parent));
                output.push_str(").mkpath\n");
                output.push_str("        libexec.install ");
                output.push_str(&ruby_quote(&file.member));
                output.push_str(" => ");
                output.push_str(&ruby_quote(&file.destination));
                output.push('\n');
                output.push_str("        (libexec/");
                output.push_str(&ruby_quote(&file.destination));
                output.push_str(").chmod(");
                output.push_str(match file.mode {
                    HomebrewInstalledMode::Data => "0644",
                    HomebrewInstalledMode::Executable => "0755",
                });
                output.push_str(")\n");
            }
            output.push_str("      end\n");
        }
        output.push_str("    end\n");
    }
    Ok(())
}

fn render_tests(output: &mut String, spec: &HomebrewFormulaSpec) -> Result<(), ContractError> {
    output.push_str("  end\n\n  test do\n");
    for bin in &spec.host.bins {
        output.push_str("    assert_match version.to_s, shell_output(\"#{bin}/");
        output.push_str(bin);
        output.push_str(" --version\", 0)\n");
    }
    for auxiliary in &spec.auxiliaries {
        for (index, target) in auxiliary.targets.iter().enumerate() {
            let files = auxiliary
                .files
                .iter()
                .filter(|file| file.target == *target)
                .collect::<Vec<_>>();
            if files.is_empty() {
                continue;
            }
            output.push_str(if index == 0 { "    if " } else { "    elsif " });
            output.push_str(ruby_cpu_predicate(target)?);
            output.push('\n');
            for file in files {
                match file.test {
                    HomebrewAuxiliaryTest::None => {}
                    HomebrewAuxiliaryTest::Executable => {
                        output.push_str("      assert_predicate libexec/ ");
                        output.push_str(&ruby_quote(&file.destination));
                        output.push_str(", :executable?\n");
                    }
                    HomebrewAuxiliaryTest::LinuxElf => {
                        let machine = match file.target.as_str() {
                            "aarch64-unknown-linux-gnu" => 183,
                            "x86_64-unknown-linux-gnu" => 62,
                            _ => return Err(invalid("linux_elf_test_target_invalid")),
                        };
                        output.push_str("      assert_predicate libexec/ ");
                        output.push_str(&ruby_quote(&file.destination));
                        output.push_str(", :executable?\n      if OS.linux?\n        assert_match version.to_s, shell_output(\"#{libexec}/");
                        output.push_str(&file.destination);
                        output.push_str(
                            " --version\", 0)\n      end\n      header = File.binread(libexec/",
                        );
                        output.push_str(&ruby_quote(&file.destination));
                        output.push_str(", 20)\n      assert_equal \"\\x7FELF\".b, header.byteslice(0, 4)\n      assert_equal 2, header.getbyte(4)\n      assert_equal 1, header.getbyte(5)\n      assert_equal ");
                        output.push_str(&machine.to_string());
                        output.push_str(", header.byteslice(18, 2).unpack1(\"v\")\n");
                    }
                }
            }
        }
        if !auxiliary.targets.is_empty() {
            output.push_str("    end\n");
        }
    }
    Ok(())
}
