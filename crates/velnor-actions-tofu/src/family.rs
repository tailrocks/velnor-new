use crate::effective::config_shape;

pub const LOCKFILE_NAME: &str = ".terraform.lock.hcl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Config,
    Override,
    Test,
    Var,
    Lock,
    Other,
}

#[must_use]
pub fn family_of(file_name: &str) -> Family {
    if file_name == LOCKFILE_NAME {
        return Family::Lock;
    }
    if let Some(shape) = config_shape(file_name) {
        return if is_override_stem(&shape.stem) {
            Family::Override
        } else {
            Family::Config
        };
    }
    if has_stem(file_name, ".tftest.hcl") || has_stem(file_name, ".tofutest.hcl") {
        return Family::Test;
    }
    if has_stem(file_name, ".tfvars") || has_stem(file_name, ".tfvars.json") {
        return Family::Var;
    }
    Family::Other
}

#[must_use]
pub fn is_override_stem(stem: &str) -> bool {
    stem == "override" || stem.ends_with("_override")
}

#[must_use]
pub fn is_auto_var(file_name: &str) -> bool {
    if file_name == "terraform.tfvars" || file_name == "terraform.tfvars.json" {
        return true;
    }
    has_stem(file_name, ".auto.tfvars") || has_stem(file_name, ".auto.tfvars.json")
}

fn has_stem(file_name: &str, suffix: &str) -> bool {
    file_name
        .strip_suffix(suffix)
        .is_some_and(|stem| !stem.is_empty())
}
