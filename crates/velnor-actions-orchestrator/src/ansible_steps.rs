// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_ansible::collection_url;
use velnor_actions_contract::Step;
use velnor_actions_contract::config::{AnsibleCheck, AnsibleCollection, AnsibleStackConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::shell_step;

use super::ansible_inventory::{AnsibleRoot, CFG_NAME};
use super::stack_support::{LockedArchive, bash, quote};
use crate::OrchestratorError;
use crate::workflow_task_jobs::stack_family_cache;

const WORK: &str = "$RUNNER_TEMP/velnor/ansible";
const COLLECTIONS_DIR: &str = "$RUNNER_TEMP/velnor/ansible-collections";
const UV_URL_CHECK: &str = "want=\"https://github.com/astral-sh/uv/releases/download/${VELNOR_TOOL_UV}/uv-x86_64-unknown-linux-gnu.tar.gz\"; \
if [ \"${url:-}\" != \"$want\" ]; then echo 'velnor: the mise.lock uv url is not the admitted archive' >&2; exit 1; fi";
const PYTHON_URL_CHECK: &str = "prefix='https://github.com/astral-sh/python-build-standalone/releases/download/'; \
rest=\"${url:-}\"; rest=\"${rest#\"$prefix\"}\"; stamp=\"${rest%%/*}\"; \
case \"$stamp\" in ''|*[!0-9]*) echo 'velnor: the mise.lock python url is not the admitted archive' >&2; exit 1 ;; esac; \
want=\"${prefix}${stamp}/cpython-${VELNOR_TOOL_PYTHON}+${stamp}-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz\"; \
if [ \"${url:-}\" != \"$want\" ]; then echo 'velnor: the mise.lock python url is not the admitted archive' >&2; exit 1; fi";

fn export_config(root: &AnsibleRoot) -> String {
    let rel = if root.dir == "." {
        CFG_NAME.to_owned()
    } else {
        format!("{}/{CFG_NAME}", root.dir)
    };
    format!(
        "export ANSIBLE_CONFIG=\"$GITHUB_WORKSPACE\"/{}",
        quote(&rel)
    )
}

fn step_env(catalog: &ToolCatalog) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let owned = |leaf: &str| format!("${{{{ runner.temp }}}}/velnor/{leaf}");
    let extra = BTreeMap::from([
        ("ANSIBLE_HOME".to_owned(), owned("ansible/home")),
        (
            "ANSIBLE_COLLECTIONS_PATH".to_owned(),
            owned("ansible/collections"),
        ),
        ("ANSIBLE_NOCOLOR".to_owned(), "1".to_owned()),
        ("UV_CACHE_DIR".to_owned(), owned("uv-cache")),
        ("UV_LINK_MODE".to_owned(), "copy".to_owned()),
        ("UV_NO_CONFIG".to_owned(), "1".to_owned()),
        ("UV_NO_PROGRESS".to_owned(), "1".to_owned()),
        ("UV_PYTHON_DOWNLOADS".to_owned(), "never".to_owned()),
    ]);
    crate::matrix_step::task_step_env(catalog, &extra, false)
}

fn archive_step(
    name: &str,
    archive: &LockedArchive<'_>,
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    Ok(shell_step(
        name,
        bash(&archive.script()),
        step_env(catalog)?,
    )?)
}

fn environment_step(
    config: &AnsibleStackConfig,
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; a=\"{WORK}\"; rm -rf \"$a/venv\"; \
\"$a/uv/uv\" venv --no-config --python \"$a/python/python/bin/python3\" \"$a/venv\"; \
\"$a/uv/uv\" pip install --no-config --require-hashes --no-deps --no-build --python \"$a/venv/bin/python\" -r {requirements}",
        requirements = quote(&config.python_requirements),
    );
    Ok(shell_step(
        "Create Python environment",
        bash(&script),
        step_env(catalog)?,
    )?)
}

fn config_step(catalog: &ToolCatalog, root: &AnsibleRoot) -> Result<Step, OrchestratorError> {
    let path = if root.dir == "." {
        CFG_NAME.to_owned()
    } else {
        format!("{}/{CFG_NAME}", root.dir)
    };
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; test -f {path}; test ! -L {path}; \
printf '%s  %s\\n' '{sha}' {path} > \"$RUNNER_TEMP/velnor-ansible-cfg.sha256\"; \
sha256sum -c \"$RUNNER_TEMP/velnor-ansible-cfg.sha256\"; rm -f \"$RUNNER_TEMP/velnor-ansible-cfg.sha256\"",
        path = quote(&path),
        sha = root.cfg_sha256,
    );
    Ok(shell_step(
        "Verify ansible.cfg",
        bash(&script),
        step_env(catalog)?,
    )?)
}

fn collection_file(collection: &AnsibleCollection) -> String {
    format!("{}-{}.tar.gz", collection.name, collection.version)
}

fn collections_step(
    collections: &[AnsibleCollection],
    catalog: &ToolCatalog,
    root: &AnsibleRoot,
) -> Result<Step, OrchestratorError> {
    let download = |collection: &AnsibleCollection| {
        let file = collection_file(collection);
        format!(
            "printf '%s  %s\\n' '{sha}' \"$c/{file}\" > \"$a/dl/{file}.sha256\"; \
if [ ! -f \"$c/{file}\" ] || ! sha256sum -c \"$a/dl/{file}.sha256\" > /dev/null 2>&1; then rm -f \"$c/{file}\"; \
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o \"$c/{file}\" {url}; \
sha256sum -c \"$a/dl/{file}.sha256\"; fi; ",
            url = quote(&collection_url(collection)),
            sha = collection.sha256,
        )
    };
    let tarballs = collections
        .iter()
        .map(|collection| format!("\"$c/{}\"", collection_file(collection)))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {export}; a=\"{WORK}\"; c=\"{COLLECTIONS_DIR}\"; rm -rf \"$a/dl\" \"$a/collections\"; mkdir -p \"$a/dl\" \"$a/collections\" \"$c\"; {downloads}\
\"$a/venv/bin/ansible-galaxy\" collection install --offline --no-deps -p \"$a/collections\" {tarballs}",
        export = export_config(root),
        downloads = collections.iter().map(download).collect::<String>(),
    );
    Ok(shell_step(
        "Install Ansible collections",
        bash(&script),
        step_env(catalog)?,
    )?)
}

fn syntax_step(
    config: &AnsibleStackConfig,
    catalog: &ToolCatalog,
    root: &AnsibleRoot,
) -> Result<Step, OrchestratorError> {
    let playbooks = root
        .playbooks
        .iter()
        .map(|playbook| quote(playbook))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "set -uo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {export}; a=\"{WORK}\"; report=\"$a/syntax-report.txt\"; : > \"$report\"; rc=0; \
for p in {playbooks}; do \
if \"$a/venv/bin/ansible-playbook\" --syntax-check -i {inventory} \"$p\" > \"$a/syntax.log\" 2>&1; then echo \"ok $p\" >> \"$report\"; \
else echo \"FAIL $p\" >> \"$report\"; cat \"$a/syntax.log\"; rc=1; fi; done; cat \"$report\"; exit \"$rc\"",
        inventory = quote(&config.inventory),
        export = export_config(root),
    );
    let mut step = shell_step("Ansible syntax check", bash(&script), step_env(catalog)?)?;
    step.parallel = true;
    Ok(step)
}

fn lint_step(
    config: &AnsibleStackConfig,
    catalog: &ToolCatalog,
    root: &AnsibleRoot,
) -> Result<Step, OrchestratorError> {
    let playbooks = root
        .playbooks
        .iter()
        .map(|playbook| quote(playbook))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {export}; a=\"{WORK}\"; \
printf '%s\\n' '---' 'profile: {profile}' 'offline: true' > \"$a/ansible-lint.yml\"; \
\"$a/venv/bin/ansible-lint\" -c \"$a/ansible-lint.yml\" {playbooks}",
        profile = config.effective_lint_profile().as_str(),
        export = export_config(root),
    );
    let mut step = shell_step("Ansible lint", bash(&script), step_env(catalog)?)?;
    step.parallel = true;
    Ok(step)
}

pub(crate) fn root_steps(
    config: &AnsibleStackConfig,
    root: &AnsibleRoot,
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, OrchestratorError> {
    let uv_dest = format!("{WORK}/uv");
    let python_dest = format!("{WORK}/python");
    let mut steps = vec![
        archive_step(
            "Install uv",
            &LockedArchive {
                key: "uv",
                version_var: "VELNOR_TOOL_UV",
                dest: &uv_dest,
                url_check: UV_URL_CHECK,
                tar_args: "--strip-components=1",
                binary: "uv",
            },
            catalog,
        )?,
        archive_step(
            "Install Python",
            &LockedArchive {
                key: "python",
                version_var: "VELNOR_TOOL_PYTHON",
                dest: &python_dest,
                url_check: PYTHON_URL_CHECK,
                tar_args: "",
                binary: "python/bin/python3",
            },
            catalog,
        )?,
    ];
    let venv = stack_family_cache::python_venv(&config.python_requirements);
    let collections = (!config.collections.is_empty())
        .then(|| stack_family_cache::ansible_collections(&config.collections));
    let mut plans = vec![venv.clone()];
    plans.extend(collections.clone());
    steps.extend(stack_family_cache::prelude(&plans)?);
    steps.push(environment_step(config, catalog)?);
    steps.extend(stack_family_cache::after_producer(&venv)?);
    steps.push(config_step(catalog, root)?);
    if let Some(collections_plan) = &collections {
        steps.push(collections_step(&config.collections, catalog, root)?);
        steps.extend(stack_family_cache::after_producer(collections_plan)?);
    }
    for check in config.effective_checks() {
        steps.push(match check {
            AnsibleCheck::Syntax => syntax_step(config, catalog, root)?,
            AnsibleCheck::Lint => lint_step(config, catalog, root)?,
        });
    }
    Ok(steps)
}
