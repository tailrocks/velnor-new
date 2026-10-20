//! `[stacks.ansible]` root jobs through the real generator and planner.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{JobDisposition, Plan};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};
use crate::impl_select::{commit, plan_pr, plan_push};
use crate::impl_select_affected_jobs::{fixture, put};

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const COLLECTION_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";
const MISE: &str = "[tools]\npython = \"3.14.7\"\nuv = \"0.13.1\"\nansible-core = \"2.21.4\"\n\"pipx:ansible-lint\" = \"26.1.1\"\n";

const ANSIBLE: &str = "
[stacks.ansible]
roots = [\"ansible-configs\"]
python_requirements = \"ansible-configs/requirements.txt\"
inventory = \"ansible-configs/hosts.ini\"
lint_profile = \"production\"

[[stacks.ansible.collections]]
name = \"ansible.posix\"
version = \"2.2.2\"
sha256 = \"89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567\"
";

fn config(extra: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra}")
}

fn lock() -> String {
    format!(
        "[[tools.python]]\nversion = \"3.14.7\"\nbackend = \"core:python\"\n\n[tools.python.\"platforms.linux-x64\"]\nchecksum = \"sha256:{HASH}\"\nurl = \"https://github.com/astral-sh/python-build-standalone/releases/download/20260901/cpython-3.14.7+20260901-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz\"\n\n\
[[tools.uv]]\nversion = \"0.13.1\"\nbackend = \"aqua:astral-sh/uv\"\n\n[tools.uv.\"platforms.linux-x64\"]\nchecksum = \"sha256:{HASH}\"\nurl = \"https://github.com/astral-sh/uv/releases/download/0.13.1/uv-x86_64-unknown-linux-gnu.tar.gz\"\n"
    )
}

fn requirements() -> String {
    format!(
        "ansible-core==2.21.4 \\\n    --hash=sha256:{HASH}\nansible-lint==26.1.1 \\\n    --hash=sha256:{HASH}\n"
    )
}

fn write_root(root: &Path) -> TestResult {
    let write = |rel: &str, text: &str| -> TestResult {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(path, text)?;
        Ok(())
    };
    write("mise.toml", MISE)?;
    write("mise.lock", &lock())?;
    write("ansible-configs/requirements.txt", &requirements())?;
    write(
        "ansible-configs/ansible.cfg",
        "[defaults]\ninventory = hosts.ini\nretry_files_enabled = False\n",
    )?;
    write("ansible-configs/hosts.ini", "[nodes]\nweb\n")?;
    write(
        "ansible-configs/requirements.yaml",
        "collections:\n  - name: ansible.posix\n    version: 2.2.2\n",
    )?;
    write("ansible-configs/site.yml", "- hosts: nodes\n  tasks: []\n")?;
    write(
        "ansible-configs/deploy.yml",
        "- hosts: nodes\n  tasks: []\n",
    )?;
    write(
        "ansible-configs/tasks/common.yml",
        "- name: x\n  debug: msg=hi\n",
    )?;
    git(&["add", "."], root)?;
    Ok(())
}

fn ci_yaml(repo: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(&prepare(repo)?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

#[test]
fn each_root_gets_one_job_with_locked_tooling_and_parallel_checks() -> TestResult {
    let repo = make_repo(&config(ANSIBLE))?;
    write_root(repo.path())?;
    let ci = ci_yaml(repo.path())?;
    let job = job_body(&ci, "ansible-ansible-configs")?;
    assert!(job.contains("name: Ansible / ansible-configs"), "{job}");
    assert!(job.contains("runs-on: ubuntu-26.04\n"), "{job}");
    assert!(job.contains("contents: read"), "{job}");
    assert!(!job.contains("services:"), "{job}");
    let order = [
        "name: Install uv",
        "name: Install Python",
        "name: Create Python environment",
        "name: Verify ansible.cfg",
        "name: Install Ansible collections",
        "name: Ansible syntax check",
        "name: Ansible lint",
        "name: Wait for parallel checks",
    ];
    let mut last = 0;
    for step in order {
        let at = job.find(step).ok_or(step)?;
        assert!(at >= last, "{step} out of order: {job}");
        last = at;
    }
    for step in ["Ansible syntax check", "Ansible lint"] {
        let after = job
            .split(&format!("- name: {step}\n"))
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .map(str::trim);
        assert_eq!(after, Some("background: true"), "{step}: {job}");
    }
    assert!(
        job.contains("VELNOR_TOOL_UV") && job.contains("VELNOR_TOOL_PYTHON"),
        "{job}"
    );
    assert!(
        job.contains("--require-hashes --no-deps --no-build"),
        "{job}"
    );
    assert!(
        job.contains("collection install --offline --no-deps"),
        "{job}"
    );
    assert!(
        job.contains("export ANSIBLE_CONFIG=") && job.contains("ansible-configs/ansible.cfg"),
        "{job}"
    );
    assert!(
        job.contains("ANSIBLE_HOME: ${{ runner.temp }}/velnor/ansible/home"),
        "{job}"
    );
    assert!(
        job.contains("ansible-configs/deploy.yml") && job.contains("ansible-configs/site.yml"),
        "{job}"
    );
    assert!(job.contains(COLLECTION_SHA), "{job}");
    assert!(job.contains("profile: production"), "{job}");
    assert!(
        !job.contains("secrets.") && !job.contains("vault-password"),
        "{job}"
    );
    let required = job_body(&ci, "required")?;
    assert!(required.contains("- ansible-ansible-configs"), "{required}");
    Ok(())
}

#[test]
fn generation_is_deterministic() -> TestResult {
    let render = || -> Result<String, Box<dyn std::error::Error>> {
        let repo = make_repo(&config(ANSIBLE))?;
        write_root(repo.path())?;
        ci_yaml(repo.path())
    };
    assert_eq!(render()?, render()?);
    Ok(())
}

fn rejected(edit: impl FnOnce(&Path) -> TestResult, code: &str) -> TestResult {
    let repo = make_repo(&config(ANSIBLE))?;
    write_root(repo.path())?;
    edit(repo.path())?;
    git(&["add", "-A"], repo.path())?;
    let error = prepare(repo.path())
        .err()
        .ok_or(format!("{code}: generation succeeded"))?;
    assert!(error.to_string().contains(code), "{code}: {error}");
    Ok(())
}

#[test]
fn contract_violations_stop_generation() -> TestResult {
    rejected(
        |root| {
            fs::write(
                root.join("ansible-configs/requirements.txt"),
                "ansible-core==2.21.4\n",
            )
            .map_err(Into::into)
        },
        "python_requirements_unhashed",
    )?;
    rejected(
        |root| {
            fs::write(root.join("mise.toml"), MISE.replace("2.21.4", "2.21.3")).map_err(Into::into)
        },
        "conflicting_project_tool_versions:ansible-core",
    )?;
    rejected(
        |root| {
            fs::write(root.join("mise.lock"), lock().replace("sha256:", "blake3:"))
                .map_err(Into::into)
        },
        "ansible_tool_checksum_missing:uv",
    )?;
    rejected(
        |root| {
            fs::write(
                root.join("ansible-configs/vars.yml"),
                "$ANSIBLE_VAULT;1.1;AES256\n6162\n",
            )
            .map_err(Into::into)
        },
        "ansible_vault_in_ci",
    )?;
    rejected(
        |root| {
            fs::write(
                root.join("ansible-configs/ansible.cfg"),
                "[defaults]\ncallback_plugins = x\n",
            )
            .map_err(Into::into)
        },
        "ansible_cfg_key_forbidden:defaults.callback_plugins",
    )?;
    rejected(
        |root| fs::remove_file(root.join("ansible-configs/ansible.cfg")).map_err(Into::into),
        "ansible_cfg_missing",
    )?;
    rejected(
        |root| {
            fs::write(
                root.join("ansible-configs/requirements.yaml"),
                "collections:\n  - name: ansible.posix\n    version: 2.2.1\n",
            )
            .map_err(Into::into)
        },
        "ansible_collections_mismatch:ansible.posix",
    )?;
    rejected(
        |root| {
            fs::write(root.join("mise.toml"), "[tools]\npython = \"3.14.7\"\n").map_err(Into::into)
        },
        "uv_not_declared",
    )
}

fn ansible_fixture() -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
    let (dir, _) = fixture(ANSIBLE, false)?;
    let root = dir.path();
    write_root(root)?;
    put(root, "other/readme.md", "x\n")?;
    let base = commit(root, "ansible")?;
    Ok((dir, base))
}

fn runs(plan: &Plan) -> BTreeSet<&str> {
    plan.jobs
        .iter()
        .filter(|job| job.job_id.starts_with("ansible-") && job.disposition == JobDisposition::Run)
        .map(|job| job.job_id.as_str())
        .collect()
}

fn pr(edit: impl FnOnce(&Path) -> TestResult) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = ansible_fixture()?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn root_requirements_and_inventory_changes_select_the_job() -> TestResult {
    let plan = pr(|root| put(root, "other/readme.md", "y\n"))?;
    assert!(runs(&plan).is_empty(), "{:?}", plan.jobs);
    assert_eq!(
        plan.jobs
            .iter()
            .filter(|job| job.job_id == "ansible-ansible-configs")
            .count(),
        1
    );
    let expect = BTreeSet::from(["ansible-ansible-configs"]);
    let plan = pr(|root| {
        put(
            root,
            "ansible-configs/tasks/common.yml",
            "- name: y\n  debug: msg=hi\n",
        )
    })?;
    assert_eq!(runs(&plan), expect);
    let (dir, base) = ansible_fixture()?;
    put(dir.path(), "other/readme.md", "z\n")?;
    let head = commit(dir.path(), "change")?;
    let (plan, _) = plan_push(dir.path(), Some(&base), &head)?;
    assert_eq!(runs(&plan), expect);
    Ok(())
}
