//! Closed domain-specific environment shape; registry equality binds all values.
use std::collections::BTreeMap;
use velnor_actions_contract::ToolCacheDomain;

const ISOLATION: [(&str, &str); 7] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
    ("RUSTUP_AUTO_INSTALL", "0"),
];
const METADATA: [&str; 4] = [
    "MISE_DATA_DIR",
    "VELNOR_MISE_SHA256",
    "VELNOR_TOOL_CACHE_IDENTITY",
    "VELNOR_QUALIFIED_TOOL_IDENTITY",
];
const GRADLE: [&str; 2] = ["JAVA_HOME", "VELNOR_GRADLE_CONSUMER_IDENTITY"];

pub(super) fn native_install_environment(
    env: &BTreeMap<String, String>,
    domain: ToolCacheDomain,
) -> bool {
    let homes = domain.home_environment();
    let extra = if domain == ToolCacheDomain::GradleBootstrap {
        &GRADLE[..]
    } else {
        &[]
    };
    env.len() == ISOLATION.len() + METADATA.len() + extra.len() + homes.len()
        && ISOLATION
            .iter()
            .all(|(key, value)| env.get(*key).map(String::as_str) == Some(*value))
        && METADATA
            .iter()
            .chain(extra)
            .all(|key| env.contains_key(*key))
        && homes.iter().all(|(key, value)| env.get(key) == Some(value))
}

#[cfg(test)]
#[path = "tool_consumer_home_tests.rs"]
mod home_tests;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn base() -> BTreeMap<String, String> {
        ISOLATION
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .chain(
                METADATA
                    .into_iter()
                    .map(|key| (key.to_owned(), "owner-bound".to_owned())),
            )
            .collect()
    }

    #[test]
    fn gradle_keys_are_required_only_in_the_gradle_domain() {
        let mut env = base();
        assert!(native_install_environment(
            &env,
            ToolCacheDomain::NpmBootstrap
        ));
        assert!(!native_install_environment(
            &env,
            ToolCacheDomain::GradleBootstrap
        ));
        env.extend(
            GRADLE
                .into_iter()
                .map(|key| (key.to_owned(), "owner-bound".to_owned())),
        );
        assert!(native_install_environment(
            &env,
            ToolCacheDomain::GradleBootstrap
        ));
        for domain in [
            ToolCacheDomain::Planning,
            ToolCacheDomain::Full,
            ToolCacheDomain::NpmBootstrap,
            ToolCacheDomain::BunBootstrap,
            ToolCacheDomain::TofuBootstrap,
        ] {
            assert!(!native_install_environment(&env, domain));
        }
    }

    #[test]
    fn altered_isolation_and_unknown_or_missing_keys_reject() {
        let mut env = base();
        env.extend(
            GRADLE
                .into_iter()
                .map(|key| (key.to_owned(), "owner-bound".to_owned())),
        );
        env.remove("JAVA_HOME");
        env.insert("UNOWNED".to_owned(), "same-count".to_owned());
        assert!(!native_install_environment(
            &env,
            ToolCacheDomain::GradleBootstrap
        ));
        env.remove("UNOWNED");
        env.insert("JAVA_HOME".to_owned(), "owner-bound".to_owned());
        env.insert("MISE_NO_HOOKS".to_owned(), "0".to_owned());
        assert!(!native_install_environment(
            &env,
            ToolCacheDomain::GradleBootstrap
        ));
    }

    #[test]
    fn rustup_auto_install_requires_exact_disable_value() {
        let mut env = base();
        env.extend(ToolCacheDomain::Full.home_environment());
        assert!(native_install_environment(&env, ToolCacheDomain::Full));
        for value in ["1", "false", "", "00"] {
            env.insert("RUSTUP_AUTO_INSTALL".to_owned(), value.to_owned());
            assert!(!native_install_environment(&env, ToolCacheDomain::Full));
        }
        env.remove("RUSTUP_AUTO_INSTALL");
        assert!(!native_install_environment(&env, ToolCacheDomain::Full));
    }
}
