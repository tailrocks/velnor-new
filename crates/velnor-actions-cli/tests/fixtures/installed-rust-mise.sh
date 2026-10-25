#!/bin/sh
set -eu
REAL_MISE=@REAL_MISE@
if [ "$#" -eq 1 ] && [ "$1" = --version ]; then
  exec "$REAL_MISE" --version
fi
[ "$#" -ge 4 ] || exit 90
[ "$1" = --no-config ] || exit 91
[ "$2" = --no-env ] || exit 92
[ "$3" = --no-hooks ] || exit 93
shift 3
case "$1" in
  ls)
    [ "$#" -eq 3 ] || exit 94
    [ "$2" = --json ] || exit 95
    [ "$3" = rust ] || exit 96
    printf '%s\n' '[{"version":"1.99.1","installed":true}]'
    ;;
  exec)
    [ "$#" -ge 5 ] || exit 97
    case "$2" in
      rust@1.99.1)
        [ "$#" -eq 10 ] || [ "$#" -eq 12 ] || exit 98
        [ "$3" = -- ] || exit 99
        shift 3
        [ "$1" = cargo ] || exit 101
        [ "$2" = metadata ] || exit 102
        [ "$3" = --format-version ] || exit 103
        [ "$4" = 1 ] || exit 104
        case "$#" in
          7)
            [ "$5" = --no-deps ] || exit 105
            [ "$6" = --manifest-path ] || exit 106
            case "$7" in
              /*/Cargo.toml) [ -f "$7" ] || exit 107 ;;
              *) exit 107 ;;
            esac
            ;;
          9)
            [ "$5" = --locked ] || exit 108
            [ "$6" = --offline ] || exit 109
            [ "$7" = --all-features ] || exit 110
            [ "$8" = --manifest-path ] || exit 111
            case "$9" in
              /*/Cargo.toml) [ -f "$9" ] || exit 112 ;;
              *) exit 112 ;;
            esac
            ;;
          *) exit 113 ;;
        esac
        exec "$@"
        ;;
      jactionlint@@JACTIONLINT_VERSION@)
        [ "$#" -ge 15 ] || exit 108
        [ "$3" = shellcheck@@SHELLCHECK_VERSION@ ] || exit 109
        [ "$4" = -- ] || exit 110
        [ "$5" = jactionlint ] || exit 111
        [ "$6" = --config-file ] || exit 112
        [ "$7" = .github/jactionlint.yaml ] || exit 113
        [ "$8" = --format ] || exit 114
        [ "$9" = oneline ] || exit 115
        [ "${10}" = --strict-exit ] || exit 116
        [ "${11}" = --no-online ] || exit 117
        [ "${12}" = --shellcheck=shellcheck ] || exit 118
        [ "${13}" = --pyflakes= ] || exit 119
        [ "${14}" = --no-color ] || exit 120
        exec "$REAL_MISE" --no-config --no-env --no-hooks "$@"
        ;;
      shellcheck@@SHELLCHECK_VERSION@)
        [ "$3" = -- ] || exit 121
        [ "$4" = shellcheck ] || exit 122
        case "$5" in
          --version)
            [ "$#" -eq 5 ] || exit 123
            ;;
          -S)
            [ "$#" -ge 8 ] || exit 124
            [ "$6" = warning ] || exit 125
            [ "$7" = --format=gcc ] || exit 126
            ;;
          --format=gcc)
            [ "$#" -ge 7 ] || exit 127
            [ "$6" = --include=SC2086 ] || exit 128
            ;;
          *)
            exit 129
            ;;
        esac
        exec "$REAL_MISE" --no-config --no-env --no-hooks "$@"
        ;;
      *)
        exit 129
        ;;
    esac
    ;;
  *)
    exit 130
    ;;
esac
