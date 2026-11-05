# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import sys

allowed = (
    "HOME", "GH_TOKEN", "MISE_DATA_DIR", "XDG_DATA_HOME", "GIT_OBJECT_DIRECTORY",
    "GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0", "GIT_CONFIG_KEY_1", "GIT_CONFIG_VALUE_1",
    "GIT_CONFIG_KEY_2", "GIT_CONFIG_VALUE_2", "GIT_CONFIG_KEY_3", "GIT_CONFIG_VALUE_3",
    "GIT_ALLOW_PROTOCOL", "GIT_NO_REPLACE_OBJECTS", "GIT_TERMINAL_PROMPT",
)
environment = {key: os.environ[key] for key in allowed if key in os.environ}
environment["PATH"] = "/usr/bin:/bin"
git_directory = os.environ["VELNOR_GIT_DIRECTORY"]
os.execve("/usr/bin/git", ["git", f"--git-dir={git_directory}", *sys.argv[1:]], environment)
