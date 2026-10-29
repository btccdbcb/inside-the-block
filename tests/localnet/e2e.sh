#!/usr/bin/env bash
# End-to-end test of the live programme on a local validator (Agave 4.3.2), with the same script the live service runs:
#   small design over the frozen targets -> deploy/run-live.sh (setup, smoke, pilot rules, Memo, sessions A-D, sweep)
#   -> collect -> verify against the local RPC -> analyse -> checks.
# Every target is mapped to the local validator's identity and each session window lasts 2 minutes
# (--rehearsal-minutes, --map-all-to; the binary refuses both on mainnet). About 20 minutes.
# Needs on PATH: solana-test-validator, solana, solana-keygen, uv; and target/release/itb (or ITB_BIN).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
WORK=${ITB_E2E_DIR:-$(mktemp -d)}
ITB_BIN=${ITB_BIN:-$REPO/target/release/itb}
RPC=http://127.0.0.1:8899
mkdir -p "$WORK"/deploy/bin "$WORK"/secrets "$WORK"/state
echo "work dir: $WORK"

solana-test-validator --ledger "$WORK/ledger" --reset --quiet --limit-ledger-size 1000000 > "$WORK/validator.log" 2>&1 &
VPID=$!
trap 'kill $VPID 2>/dev/null || true' EXIT
for _ in $(seq 1 90); do solana -u $RPC cluster-version > /dev/null 2>&1 && break; sleep 1; done
IDENTITY=$(solana-keygen pubkey "$WORK/ledger/validator-keypair.json")

solana-keygen new --no-bip39-passphrase --silent --outfile "$WORK/secrets/funder.json" > /dev/null
chmod 600 "$WORK/secrets/funder.json"
solana -u $RPC airdrop 20 "$(solana-keygen pubkey "$WORK/secrets/funder.json")" > /dev/null

if command -v itb-analysis > /dev/null; then ana() { itb-analysis "$@"; }
else ana() { (cd "$REPO/analysis" && uv run --frozen --quiet itb-analysis "$@"); }; fi
ana design --dir "$WORK/deploy/design" --targets "$REPO/design/targets.json" \
    --allocation "$REPO/tests/localnet/allocation.json" --seed 11 > /dev/null
cp "$ITB_BIN" "$WORK/deploy/bin/itb"
cp "$REPO/deploy/run-live.sh" "$WORK/deploy/run-live.sh"
(git -C "$REPO" rev-parse --short HEAD 2> /dev/null || echo unknown) > "$WORK/deploy/COMMIT"

ITB_DEPLOY_DIR="$WORK/deploy" ITB_STATE_DIR="$WORK/state" ITB_SECRETS_DIR="$WORK/secrets" \
ITB_FUNDER_KEY="$WORK/secrets/funder.json" ITB_RPC_URLS=$RPC ITB_WS_URLS=ws://127.0.0.1:8900 \
ITB_CLOCK_EVENT=createdBank ITB_LIVE_EXTRA_ARGS="--allow-non-mainnet --rehearsal-minutes 2 --map-all-to $IDENTITY" \
    bash "$WORK/deploy/run-live.sh"
if [[ ! -e "$WORK/state/DONE" ]]; then echo "FAIL: the programme did not finish"; tail -20 "$WORK/state/live.log"; exit 1; fi

# collect at once (the local validator prunes old blocks), then verify and analyse
ana collect "$WORK"/state/runs/2* --slots "$WORK"/state/runs/slots/*.jsonl --out "$WORK/raw" --rpc $RPC
ana analyse "$WORK/raw" --targets "$WORK/deploy/design/targets.json" --out "$WORK/results" --n-perm 199
ana verify "$WORK/raw" --pairs "$WORK/results/pairs.csv" --rpc $RPC --out "$WORK/verify.json"

# every probe landed with no completeness problem; G2 timing holds; ledger = chain; every owned account is empty
python3 - "$WORK" "$RPC" <<'PY'
import json, sys, urllib.request
from pathlib import Path
work, rpc = Path(sys.argv[1]), sys.argv[2]
fails = []
for run in sorted((work / "raw").iterdir()):
    m = json.loads((run / "run.json").read_text())
    if m["landed"] != m["probes"] or m["completeness_problems"]:
        fails.append(f"{run.name}: {m['landed']}/{m['probes']} landed, {len(m['completeness_problems'])} problems")
log = [json.loads(line) for line in (work / "state/live.log").read_text().splitlines() if line.startswith("{")]
sweep = [e for e in log if e.get("event") == "stage_done" and e.get("stage") == "sweep"]
if not sweep or sweep[-1]["detail"]["charged_total"] != sweep[-1]["detail"]["landed_fees"]:
    fails.append("ledger charges differ from the landed fees")
recs = [e for e in log if e.get("event") == "reconcile"]
if not recs or not recs[-1]["clean"]:
    fails.append("last reconciliation not clean")
for e in (e for e in log if e.get("event") == "account_registered"):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [e["pubkey"]]}).encode()
    req = urllib.request.Request(rpc, body, {"Content-Type": "application/json"})
    bal = json.loads(urllib.request.urlopen(req).read())["result"]["value"]
    if bal:
        fails.append(f"{e['role']} {e['pubkey']} still holds {bal} lamports")
print("\n".join(fails) if fails else "localnet e2e: all checks pass")
sys.exit(1 if fails else 0)
PY
