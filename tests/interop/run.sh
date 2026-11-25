#!/usr/bin/env bash
# End-to-end interop tests: four headless MegaSIP instances (`megasip --cli`, no sound
# devices; each "microphone" is a distinct test tone) against a private Asterisk on
# 127.0.0.1:5070. Needs `asterisk` installed; the system service can stay off.
#
#   tests/interop/run.sh                 # all tests
#   tests/interop/run.sh t_srtp t_conference
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$HERE/../.."
WORK="$(mktemp -d "${TMPDIR:-/tmp}/megasip-interop.XXXXXX")"
export INTEROP_WORK="$WORK"
# Throwaway test accounts must not land in the real wallet.
export MEGASIP_NO_KEYRING=1
A="$WORK/ast"
mkdir -p "$A"/{etc,run,log,spool,lib} "$WORK/phones"

cleanup() {
    asterisk -C "$A/etc/asterisk.conf" -rx "core stop now" >/dev/null 2>&1 || true
    [ -n "${KEEP:-}" ] || rm -rf "$WORK"
}
trap cleanup EXIT

if ss -lun 2>/dev/null | grep -q '127.0.0.1:5070 '; then
    echo "Port 5070 is already in use (another test Asterisk?). Stop it first." >&2
    exit 1
fi

cargo build --manifest-path "$ROOT/Cargo.toml" -q

cp "$HERE"/asterisk/*.conf "$A/etc/"
cat > "$A/etc/asterisk.conf" <<CONF
[directories]
astetcdir => $A/etc
astmoddir => $(dirname "$(find /usr/lib -name chan_pjsip.so -print -quit)")
astvarlibdir => $A/lib
astdbdir => $A/lib
astkeydir => $A/lib
astdatadir => /usr/share/asterisk
astagidir => $A/lib/agi-bin
astspooldir => $A/spool
astrundir => $A/run
astlogdir => $A/log
astsbindir => /usr/sbin
[options]
verbose = 3
nofork = yes
CONF
(cd "$A" && asterisk -C "$A/etc/asterisk.conf" -f -vvv > "$A/log/console.txt" 2>&1 &)
for _ in $(seq 1 30); do
    asterisk -C "$A/etc/asterisk.conf" -rx "core waitfullybooted" >/dev/null 2>&1 && break
    sleep 0.5
done

phone() { # name user password transport srtp codecs
    mkdir -p "$WORK/phones/$1"
    cat > "$WORK/phones/$1/settings.json" <<JSON
{"accounts":[{"id":"a$2","label":"$1","server":"127.0.0.1:5070","username":"$2","password":"$3","displayName":"$1","transport":"$4","srtp":"$5"}],"codecs":$6}
JSON
}
phone alice 1001 secret1 udp off '["opus","g722","pcmu","pcma"]'
phone bob   1002 secret2 tcp off '["g722","pcmu"]'
phone carol 1003 secret3 udp off '["opus","pcmu"]'
phone dave  1004 secret4 udp mandatory '["pcmu"]'

MEGASIP_BIN="$ROOT/target/debug/megasip" python3 -I "$HERE/harness.py" "$@"
