#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Scratch repositories must not execute caller hooks or inherit Git authority.
# ROOT index/archive reads deliberately do not use this helper.
fixture_git() (
  local key
  for key in ${!GIT_@}; do unset "$key"; done
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL=/dev/null
  export GIT_TEMPLATE_DIR=""
  export GIT_AUTHOR_NAME=v GIT_AUTHOR_EMAIL=v@v GIT_COMMITTER_NAME=v GIT_COMMITTER_EMAIL=v@v
  export GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z'
  # LFS clean/smudge remain explicit: source bytes must retain their normal
  # pointer semantics even though ambient filters/configuration are excluded.
  command git -c core.hooksPath=/dev/null -c commit.gpgsign=false -c init.defaultBranch=main \
    -c 'filter.lfs.clean=git-lfs clean -- %f' \
    -c 'filter.lfs.smudge=git-lfs smudge -- %f' \
    -c 'filter.lfs.process=git-lfs filter-process' -c filter.lfs.required=true "$@"
)
