//! Evasion fixture: raw shell string must be flagged.

fn run_evil(script: &str) {
    let shell = "sh -c";
    let argv = [shell, script];
    let _ = argv;
}
