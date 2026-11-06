// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const PREPARE_UNITS: &str = r#"command -v printenv >/dev/null 2>&1 || exit 1
newline=$(printf '\nx')
newline=${newline%x}
read_env() {
  result=$(printenv "$1" && printf .) || return 1
  case "$result" in *"$newline".) result=${result%"$newline".} ;; *) return 1 ;; esac
}
valid_digest() {
  [ "${#1}" -eq 69 ] || return 1
  case "$1" in s256-*[!0-9a-f]*) return 1 ;; s256-*) return 0 ;; *) return 1 ;; esac
}
i=0
while [ "$i" -lt "$VELNOR_STATE_SELECT_UNIT_COUNT" ]; do
  record_name="VELNOR_STATE_SELECT_UNIT_R$i"
  read_env "$record_name" || exit 1
  record=$result
  [ "${#record}" -le 512 ] || exit 1
  case "$record" in *" "*) prefix=${record%% *}; rest=${record#* } ;; *) exit 1 ;; esac
  case "$rest" in *" "*) attested=${rest%% *}; produce=${rest#* } ;; *) exit 1 ;; esac
  case "$produce" in *" "*|"") exit 1 ;; esac
  case "$prefix" in
    "$state_prefix"union-*) unit_name=union; root=${prefix#"$state_prefix"union-} ;;
    "$state_prefix"suite-*) unit_name=suite; root=${prefix#"$state_prefix"suite-} ;;
    *) exit 1 ;;
  esac
  case "$root" in *-) root=${root%-} ;; *) exit 1 ;; esac
  case "$root" in
    ""|[!abcdefghijklmnopqrstuvwxyz0123456789]*|*[!abcdefghijklmnopqrstuvwxyz0123456789-]*) exit 1 ;;
  esac
  [ "${#root}" -le 48 ] || exit 1
  case "-$root-" in *-s256-*) exit 1 ;; esac
  slug=
  remaining=$root
  while [ -n "$remaining" ]; do
    character=${remaining%"${remaining#?}"}
    remaining=${remaining#?}
    case "$character" in -) slug="${slug}_" ;; *) slug="${slug}${character}" ;; esac
  done
  [ "$attested" = "state_${unit_name}_${slug}_attested" ] || exit 1
  [ "$produce" = "state_${unit_name}_${slug}_produce" ] || exit 1
  compat_name="VELNOR_STATE_COMPAT_R$i"
  snapshot_name="VELNOR_STATE_SNAPSHOT_R$i"
  compat=
  snapshot=
  if read_env "$compat_name"; then compat=$result; fi
  if read_env "$snapshot_name"; then snapshot=$result; fi
  valid_digest "$compat" || compat=
  valid_digest "$snapshot" || snapshot=
  unit "$prefix" "r$i" "$attested" "$produce" "$compat" "$snapshot"
  i=$((i + 1))
done
[ -e "$d/units" ] || : > "$d/units"
"#;

pub(super) const REMOVE_UNIT_ENV: &str = r#"i=0
while [ "$i" -lt "$VELNOR_STATE_SELECT_UNIT_COUNT" ]; do
  unset "VELNOR_STATE_SELECT_UNIT_R$i" "VELNOR_STATE_COMPAT_R$i" "VELNOR_STATE_SNAPSHOT_R$i"
  i=$((i + 1))
done
unset VELNOR_STATE_SELECT_UNIT_COUNT
"#;

pub(super) const COLD_UNITS: &str = r#"i=0
while [ "$i" -lt "$VELNOR_STATE_SELECT_UNIT_COUNT" ]; do
  record_name="VELNOR_STATE_SELECT_UNIT_R$i"
  read_env "$record_name" || exit 1
  record=$result
  prefix=${record%% *}
  rest=${record#* }
  attested=${rest%% *}
  produce=${rest#* }
  compat_name="VELNOR_STATE_COMPAT_R$i"
  snapshot_name="VELNOR_STATE_SNAPSHOT_R$i"
  compat=
  snapshot=
  if read_env "$compat_name"; then compat=$result; fi
  if read_env "$snapshot_name"; then snapshot=$result; fi
  valid_digest "$compat" || compat=
  valid_digest "$snapshot" || snapshot=
  emit "$attested" "r$i"
  decide "$produce" "r$i" "$prefix" "$compat" "$snapshot"
  i=$((i + 1))
done
"#;
