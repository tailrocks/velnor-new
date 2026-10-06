//! `velnor-host` process entry. Scheduling stays in the host crate.

fn main() -> std::process::ExitCode {
    velnor_runner_cli::run()
}
