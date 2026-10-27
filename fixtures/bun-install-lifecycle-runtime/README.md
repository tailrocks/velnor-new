# Bun install lifecycle runtime fixture

This fixture uses trusted local `file:` dependencies. One dependency has a `postinstall` script that records the selected runtime; another provides a local `bun` executable for shadowing tests. Its lockfile was generated once with Bun 1.4.4 and `--ignore-scripts`. It has no registry dependencies or credentials.
