use super::{
    DEFAULT_COOLDOWN_DAYS, MAX_COOLDOWN_DAYS, MIN_COOLDOWN_DAYS, check_policy_header,
    policy_mise_version, toolchain_specs,
};
use crate::context::FreshnessContext;
use std::fs;
use std::path::{Path, PathBuf};

fn eval_policy_header(extra_header: &str) -> FreshnessContext {
    let toml_text = format!(
        "schema = 1\nchannel = \"stable\"\nregistry = \"https://github.com/rust-lang/crates.io-index\"\ncheck_interval_hours = 24\nmax_exception_days = 14\n{extra_header}"
    );
    let mut ctx = FreshnessContext::new(PathBuf::from("."), false, false);
    ctx.policy = toml::from_str(&toml_text).ok();
    check_policy_header(&mut ctx);
    ctx
}

