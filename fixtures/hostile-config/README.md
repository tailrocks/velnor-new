# hostile-config fixture
Intent: adversarial .velnor/config.toml.
Contains: unknown keys (extra_unknown_key,
unknown_future_key, [evil]), bad types (jobs as
string, ignore as int), traversal (../..),
absolute (/absolute/path) excludes, 100KB value.
Expected detector outcome: REJECTED at config
validation with unknown_config_field / invalid
exclude errors before any output.
