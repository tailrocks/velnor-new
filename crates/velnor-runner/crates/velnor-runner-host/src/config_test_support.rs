pub(crate) fn connection(
    id: &str,
    repository_id: i64,
    repository: &str,
    account: &str,
    scale_set: &str,
    source_policy: &str,
) -> String {
    format!(
        "[[connections]]\nconnection_id = \"{id}\"\nrepository_id = {repository_id}\nrepository = \"{repository}\"\ncredential_ref = \"keychain:com.tailrocks.velnor.host/{account}\"\n[connections.scale_set]\nname = \"{scale_set}\"\nlabels = [\"{scale_set}\", \"velnor\"]\n[connections.source_policy]\nmode = \"branch_restricted\"\n{source_policy}\n"
    )
}

pub(crate) fn basic_connection(
    id: &str,
    repository_id: i64,
    repository: &str,
    account: &str,
    scale_set: &str,
) -> String {
    connection(
        id,
        repository_id,
        repository,
        account,
        scale_set,
        "branches = [\"sea/integration\"]\ntag_prefixes = []\nevents = [\"push\"]\npull_request_rules = []",
    )
}

pub(crate) fn host_config(connections: &[String], max_jobs: u32) -> String {
    format!(
        "schema = 2\n{}\n[host]\nmax_jobs = {max_jobs}\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 2147483648\ndind_cpu_millicores = 3000\ndind_memory_bytes = 6442450944\noperational_memory_reserve_bytes = 1073741824\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n",
        connections.join("\n")
    )
}
