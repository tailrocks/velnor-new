# mbx-nextest fixture
Intent: workspace with MBX + Nextest executable intent.
Signals: mise.toml mr_boxington=true; executable
.mise/tasks/test invoking `cargo nextest run`.
Expected detector outcome: profile compile_driver=mbx,
test_runner=cargo_nextest. (Installed tools alone would
NOT count; here intent is executable.)
