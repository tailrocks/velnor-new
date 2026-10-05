# nested fixture
Intent: nested workspaces — root workspace (crates/a)
plus nested/lib with its own Cargo.toml; path
dev-dependency from a -> lib.
Expected detector outcome: registry finds each
workspace exactly once; nested/lib detected as its
own workspace, not double-counted.
