#!/bin/bash
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$SCRATCH/keychain-watch.log"
status() {
  python3 - <<'PY'
import ctypes
Security = ctypes.CDLL("/System/Library/Frameworks/Security.framework/Security")
Security.SecKeychainCopyDefault.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
Security.SecKeychainCopyDefault.restype = ctypes.c_int
Security.SecKeychainGetStatus.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
Security.SecKeychainGetStatus.restype = ctypes.c_int
kc = ctypes.c_void_p()
rc = Security.SecKeychainCopyDefault(ctypes.byref(kc))
st = ctypes.c_uint32()
rc2 = Security.SecKeychainGetStatus(kc, ctypes.byref(st))
word = "unlocked" if (st.value & 1) else "locked"
print(f"rc={rc}/{rc2} bits=0x{st.value:x} {word}")
PY
}
while true; do
  st="$(status 2>>"$LOG")"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$st" >>"$LOG"
  case "$st" in
    *unlocked*)
      echo "ACTION_REQUIRED: login keychain unlocked ($st)"
      exit 0
      ;;
  esac
  pid="$(pgrep -x velnor-host || true)"
  if [ -n "$pid" ]; then
    echo "ACTION_REQUIRED: velnor-host running pid $pid"
    exit 0
  fi
  sleep 30
done
