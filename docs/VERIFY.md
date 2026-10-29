# Checking the landing claims yourself

Every result in REPORT.md rests on one kind of claim: probe *s* landed (or did not land) at index *i* of block *n*,
whose leader was the target. `itb-analysis verify` re-checks those claims. It needs no keys. It does not use the
sender's code: the probe parameters are restated in `analysis/itb_analysis/verify.py` from `docs/PREREGISTRATION.md`
§2, and the transactions are decoded by `analysis/itb_analysis/txdecode.py`, a separate decoder in Python.

## Run it

```
docker compose --profile verify up          # 60 claims, seeded sample, against public mainnet RPC
```

or without Docker, from `analysis/`:

```
uv run itb-analysis verify ../data/raw --pairs ../results/pairs.csv                         # every claim, offline
uv run itb-analysis verify ../data/raw --sample 60 --seed 1 --rpc https://api.mainnet-beta.solana.com
```

It prints `VERIFY PASS` or `VERIFY FAIL` with the failing signatures and exits non-zero on failure. `--out` writes the
result as JSON.

## What it checks

For a probe claimed **landed**, offline, from the archived block (`data/raw/<run>/blocks/<slot>.json.gz`):

1. The archived bytes decode. Every signature verifies (ed25519). The first signature is the claimed one.
2. The bytes equal the signed bytes the sender logged before sending (`sends.jsonl`), so the transaction was not
   changed between signing and landing.
3. The compute-budget instructions request exactly the planned probe at the quad's fee scale: CU limit, CU price
   (legacy) or priority fee (v1), and for v1 the 64 MiB loaded-data limit.
4. The only value moved is 1 lamport from the planned fee payer to the planned recipient.
5. `meta.fee` = 5,000 + priority fee, `meta.costUnits` = the predicted value (1,784 / 1,484 / 1,481, derived in
   PREREGISTRATION §9), and no error.
6. The block's signature list has this signature at the claimed index.
7. A probe counted as landing in the target's turn sits in a block whose Fee reward went to the target.

For a probe claimed **not landed**:

8. Its signature (first 4 bytes) is in none of the archived blocks. The archive holds the target's whole turn and the
   preceding leader's turn for every quad.

With `--rpc`, the same claims are checked against a public RPC instead of my archive:

- landed: the block is fetched again and holds this transaction, byte for byte, at the claimed index, with the same
  Fee-reward recipient;
- not landed: `getSignatureStatuses` with full history search returns nothing;
- for every sampled quad, the first block of the target's turn as archived (signature list and leader) equals the
  block fetched again.

With `--pairs results/pairs.csv`, the order of every coded pair (which member came first) is recomputed from the
(slot, index) of its two members.

## The Memo and the ledger

`itb-analysis memo-check` (one RPC call) fetches the pre-registration Memo named in `data/live/ledger.csv` and
checks that it landed without error, paid by the campaign funder; that its text holds the sha256 of
`design/design.jsonl`, `design/campaign.json`, `docs/PREREGISTRATION.md` and `data/live/pilot_decision.json` as
committed; that `campaign.json` records the same pre-registration hash; and that its block is earlier than the first
campaign send (block time) and than the first slot of every campaign turn. It also lists the confirmatory-code files
whose hash differs from the one recorded at the freeze (each such change is a deviation, REPORT §9).

`itb-analysis ledger-check` lists, for every account in `data/live/live.log`, every transaction after its
registration, and checks that three totals agree to the lamport: the ledger's landed fees, the `meta.fee` of every
transaction paid by one of these accounts, and the balance change (balances at registration − balances now − lamports
that left for other addresses + lamports that arrived). It also lists landed ledger rows the chain does not have,
transactions the ledger does not have, and accounts whose on-chain changes do not add up to their balance.

Both run in `docker compose --profile verify up` once `data/live/` exists, and in the localnet end-to-end test
(`tests/localnet/e2e.sh`, every CI dispatch of the `localnet` job).

## Why the expected values are what they are

- The fee formula (5,000 lamports per signature plus ⌈CU limit × CU price / 10⁶⌉) and the cost formula are read from
  Agave v4.3.2; the files are listed in PREREGISTRATION §9.
- The three costUnits values are derived there from that formula, then were measured on a local validator (Agave
  4.3.2): every landed probe had exactly these values.
- The leader of a block is the account that received its `Fee` reward in `getBlock(..., rewards: true)`. This is the
  validator's identity, independent of any leader-schedule call I made at send time.

## What it was run on before the campaign

| Data | Claims | Result | When |
|---|---|---|---|
| Localnet rehearsal (`solana-test-validator`, Agave 4.3.2), all runs of one full programme | 204 landed + pair order of 36 pairs | 0 failures | 2026-10-29 |
| Mainnet dry run, 8 quads (nothing sent), checked against api.mainnet-beta.solana.com | 32 not landed + 8 turn blocks re-fetched | 0 failures | 2026-10-29 |
| Unit tests (`analysis/tests/test_verify.py`): a changed fee, one flipped byte, a wrong leader, a hidden probe | — | each one detected | every CI run |

The results on the campaign data are in REPORT.md.
