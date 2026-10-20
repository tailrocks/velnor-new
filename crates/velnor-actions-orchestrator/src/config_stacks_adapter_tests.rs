//! Strict loading of the dbt and Ansible stack tables.

use crate::config::load_config;

fn rooted(body: &str) -> tempfile::TempDir {
    super::tests::rooted(body)
}

#[test]
fn dbt_stack_loads_strictly_and_requires_projects() {
    let root = rooted(
        "schema = 1\n[stacks.dbt]\nprojects = [\"chainargos-dbt\"]\nservices = [\"postgres\"]\nchecks = [\"parse\", \"compile\"]\n[stacks.dbt.env]\nDBT_HOST = \"127.0.0.1\"\n",
    );
    let dbt = load_config(root.path())
        .expect("dbt stack")
        .stacks
        .dbt
        .expect("dbt");
    assert_eq!(dbt.projects[0].as_str(), "chainargos-dbt");
    assert_eq!(dbt.env["DBT_HOST"], "127.0.0.1");
    for body in [
        "schema = 1\n[stacks.dbt]\n",
        "schema = 1\n[stacks.dbt]\nprojects = []\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\nrun = \"dbt run\"\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\nchecks = [\"compile\"]\n",
        "schema = 1\n[stacks.dbt]\nprojects = [\".\"]\n[stacks.dbt.env]\nHOME = \"x\"\n",
    ] {
        assert!(load_config(rooted(body).path()).is_err(), "{body}");
    }
}

#[test]
fn ansible_stack_loads_strictly_and_requires_its_inputs() {
    let sha = "a".repeat(64);
    let root = rooted(&format!(
        "schema = 1\n[stacks.ansible]\nroots = [\"ansible-configs\"]\npython_requirements = \"ansible-configs/requirements.txt\"\ninventory = \"ansible-configs/hosts.ini\"\n[[stacks.ansible.collections]]\nname = \"ansible.posix\"\nversion = \"2.2.2\"\nsha256 = \"{sha}\"\n"
    ));
    let ansible = load_config(root.path())
        .expect("ansible stack")
        .stacks
        .ansible
        .expect("ansible");
    assert_eq!(ansible.roots[0].as_str(), "ansible-configs");
    assert_eq!(ansible.collections[0].name, "ansible.posix");
    for body in [
        "schema = 1\n[stacks.ansible]\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\npython_requirements = \"r.txt\"\ninventory = \"../h\"\n",
        "schema = 1\n[stacks.ansible]\nroots = [\"a\"]\npython_requirements = \"r.txt\"\ninventory = \"h\"\nplaybook = \"x\"\n",
    ] {
        assert!(load_config(rooted(body).path()).is_err(), "{body}");
    }
}
