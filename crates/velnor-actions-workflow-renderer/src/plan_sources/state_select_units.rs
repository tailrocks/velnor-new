// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const PREPARE_UNITS: &str = r#"command -v printenv >/dev/null 2>&1 || exit 1
read_env() {
  result=
  if ! printenv "$1" > "$d/value"; then return 1; fi
  { IFS= read -r result || return 1; extra=; if IFS= read -r extra; then return 1; fi; [ -z "$extra" ] || return 1; } < "$d/value"
}
valid_digest() {
  [ "${#1}" -eq 69 ] || return 1
  case "$1" in s256-*[!0-9a-f]*) return 1 ;; s256-*) return 0 ;; *) return 1 ;; esac
}
seen=
for i in $ordinals; do
  record_name="VELNOR_STATE_SELECT_UNIT_R$i"
  read_env "$record_name" || exit 1
  record=$result
  [ "${#record}" -le 512 ] || exit 1
  case "$record" in *" "*) prefix=${record%% *}; rest=${record#* } ;; *) exit 1 ;; esac
  case "$rest" in *" "*) attested=${rest%% *}; produce=${rest#* } ;; *) exit 1 ;; esac
  case "$produce" in *" "*|"") exit 1 ;; esac
  case "$prefix" in "$state_prefix"union-*) unit_name=union; root=${prefix#"$state_prefix"union-} ;; "$state_prefix"suite-*) unit_name=suite; root=${prefix#"$state_prefix"suite-} ;; *) exit 1 ;; esac
  case "$root" in *-) root=${root%-} ;; *) exit 1 ;; esac
  case "$root" in ""|[!abcdefghijklmnopqrstuvwxyz0123456789]*|*[!abcdefghijklmnopqrstuvwxyz0123456789-]*) exit 1 ;; esac
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
  seen=${seen}x
done
[ "${#seen}" -eq "$VELNOR_STATE_SELECT_UNIT_COUNT" ] || exit 1
[ -e "$d/units" ] || : > "$d/units"
"#;

pub(super) const REMOVE_UNIT_ENV: &str = r#"for i in $ordinals; do
  unset "VELNOR_STATE_SELECT_UNIT_R$i" "VELNOR_STATE_COMPAT_R$i" "VELNOR_STATE_SNAPSHOT_R$i"
done
unset VELNOR_STATE_SELECT_UNIT_COUNT
unset ordinals
"#;

pub(super) const COLD_UNITS: &str = r#"exec 3< "$d/units"
for i in $ordinals; do
  line=
  IFS= read -r line <&3 || exit 1
  case "$line" in *" "*) prefix=${line%% *}; rest=${line#* } ;; *) exit 1 ;; esac
  case "$rest" in *" "*) ordinal=${rest%% *}; rest=${rest#* } ;; *) exit 1 ;; esac
  [ "$ordinal" = "r$i" ] || exit 1
  case "$rest" in *" "*) attested=${rest%% *}; rest=${rest#* } ;; *) exit 1 ;; esac
  case "$rest" in *" "*) produce=${rest%% *}; rest=${rest#* } ;; *) exit 1 ;; esac
  case "$rest" in "  ") compat=; snapshot= ;; " "*) compat=; snapshot=${rest# } ;; *" "*) compat=${rest%% *}; snapshot=${rest#* } ;; *) exit 1 ;; esac
  case "$snapshot" in *" "*) exit 1 ;; esac
  emit "$attested" "$ordinal"
  decide "$produce" "$ordinal" "$prefix" "$compat" "$snapshot"
done
line=
if IFS= read -r line <&3 || [ -n "$line" ]; then exit 1; fi
exec 3<&-
"#;
