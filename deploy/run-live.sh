#!/usr/bin/env bash
# The live programme, run by itb-live.service (user itblive). It is one command, `itb live --live`, which records
# every finished stage in the ledger and resumes at the first unfinished one after a restart.
#
#   exit code 0  programme finished (DONE marker) or nothing to do
#   exit code 3  a coded check stopped the programme: write the STOP marker, exit 0 so systemd does not restart it
#   other        transient failure: systemd restarts after 30 s (Restart=on-failure); 60 failures in a row -> STOP
set -euo pipefail

: "${ITB_STATE_DIR:?}" "${ITB_DEPLOY_DIR:?}" "${ITB_SECRETS_DIR:?}" "${ITB_FUNDER_KEY:?}"
STOP="$ITB_STATE_DIR/STOP"
DONE="$ITB_STATE_DIR/DONE"
FAILS="$ITB_STATE_DIR/consecutive_failures"
LOG="$ITB_STATE_DIR/live.log"

say() { printf '{"utc":"%s","event":"run_live","msg":"%s"}\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)" "$*" | tee -a "$LOG" >&2; }

if [[ -e "$STOP" ]]; then say "STOP marker present ($(head -c 200 "$STOP")): not running"; exit 0; fi
if [[ -e "$DONE" ]]; then say "programme finished at $(cat "$DONE"): nothing to do"; exit 0; fi

n=$(cat "$FAILS" 2>/dev/null || echo 0)
if (( n >= 60 )); then
    echo "60 consecutive failures, last at $(date -u +%FT%TZ)" > "$STOP"
    say "too many consecutive failures: STOP"
    exit 0
fi

say "starting itb live (commit $(head -n1 "$ITB_DEPLOY_DIR/COMMIT" 2>/dev/null || echo unknown), failures so far $n)"
start=$(date +%s)
set +e
"$ITB_DEPLOY_DIR/bin/itb" live --live --max-spend-sol 0.03 \
    --design "$ITB_DEPLOY_DIR/design" --state "$ITB_STATE_DIR" \
    --secrets "$ITB_SECRETS_DIR" --funder-key "$ITB_FUNDER_KEY" 2>> "$LOG"
rc=$?
set -e
ran=$(( $(date +%s) - start ))

case $rc in
    0)
        echo 0 > "$FAILS"
        say "programme finished"
        exit 0 ;;
    3)
        echo "stopped by a coded check at $(date -u +%FT%TZ) (see live.log)" > "$STOP"
        say "stopped by a coded check: STOP marker written"
        exit 0 ;;
    *)
        # a run that made progress for 10 minutes resets the failure count
        if (( ran > 600 )); then n=0; fi
        echo $(( n + 1 )) > "$FAILS"
        say "itb live failed with code $rc after ${ran}s; systemd restarts it"
        exit "$rc" ;;
esac
