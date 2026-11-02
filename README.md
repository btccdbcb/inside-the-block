# Inside the Block

What a Solana leader does with two of my transactions that write the same accounts, depending on its client, on when in
the hand-over they arrive, on the gap between them and on how full the block is. Measured on mainnet with my own probes,
sent straight to each leader's TPU port at randomised moments of its turn.

**What I found.** Transactions that reach a leader before its turn wait, and are then taken in order of reward per
cost: the higher-paying member of a pair went first in 80–96 % of such pairs, whichever I sent first, and where fee,
CU price and reward per cost disagreed, reward per cost decided. During the turn the leader works in short batches: a
higher key overtakes a lower one sent a few milliseconds earlier, but never one sent 50 ms or more earlier. Of two
identical transactions, Firedancer usually takes the later arrival first (the earlier one went first in
<!-- v:results.P1.Firedancer|prop -->0.18 [0.11, 0.26] (18/102)<!-- /v --> of pairs) and JitoLabs the earlier one
(<!-- v:results.P1.JitoLabs|prop -->0.73 [0.60, 0.84] (41/56)<!-- /v -->); for the other clients the share is not
distinguishable from 0.5, or the pairs are too few. When in the hand-over I sent, from 300 ms before to 900 ms after the first shred of the slot
before the turn, did not change whether a probe landed in the target's turn or with the leader before it, in any
client. Whether block load changes the order is inconclusive. Details and tests: [REPORT.md](REPORT.md).

<!-- v:n.identities|int -->14<!-- /v --> validators, <!-- v:n.families|int -->9<!-- /v --> clients as named in gossip,
<!-- v:n.probes|int -->2,992<!-- /v --> probes in <!-- v:n.quads_fired|int -->748<!-- /v --> quads, mainnet
<!-- v:n.first_day -->2026-10-30<!-- /v --> to <!-- v:n.last_day -->2026-11-02<!-- /v --> (UTC), fees
<!-- v:n.fees_sol|.4f -->0.0155<!-- /v --> SOL.

<!-- TODO(after D): re-check the claims above against the final tables (every client with enough data; never at 50 ms). -->

## The turn

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="figures/turn-map-dark.svg">
  <img alt="Per client family, the share of probes that landed in the target's turn and the share that landed with the preceding leader, against the send offset" src="figures/turn-map-light.svg">
</picture>

Share of probes that landed in the target's turn (blue) and with the leader before it (orange), against when I sent
them, with 95 % intervals. Tests P2 and S3: [REPORT §4](REPORT.md#4-position-in-the-leaders-turn).

## Validators measured

<!-- table:validators -->
*Source: gossip views in data/raw/*/quads.jsonl; results/results.json → descriptive.per_target.*

| slot | validator | client (gossip) | versions seen | ingress | F, equal keys | R: higher key first | probes in its turn |
|---|---|---|---|---|---|---|---|
| T1 | [mds4…cT6Z](https://explorer.solana.com/address/mds4GEuiSgQRqveGyktWpETBFCb4AS2wDnhqwLHcT6Z) | Agave | 4.4.0, 4.4.0-rc.1 | direct | 0.78 [0.52, 0.94] (14/18) | 0.42 [0.15, 0.72] (5/12) | 1.00 [0.96, 1.00] (92/92) |
| T2 | [9rkJ…NDSQ](https://explorer.solana.com/address/9rkJMARqK6VBkcxGfKBAwnA44gPAfGxPbPsfsggFNDSQ) | AgaveBam | 4.3.2, 4.4.0 | shared | 0.33 [0.01, 0.91] (1/3) | 0.33 [0.01, 0.91] (1/3) | 1.00 [0.99, 1.00] (280/280) |
| T3 | [3SkE…NzXd](https://explorer.solana.com/address/3SkE34PVeGck2ArEffFKjihrQgURsvnoTAhitsNXNzXd) | JitoLabs | 4.3.2 | direct | 0.75 [0.62, 0.86] (40/53) | 0.63 [0.44, 0.80] (19/30) | 1.00 [0.98, 1.00] (271/272) |
| T4 | [5Cch…q9Ux](https://explorer.solana.com/address/5Cchr1XGEg7dbBXByV5NY2ad8jfxAM7HA3x8D56rq9Ux) | AgaveBam | 4.3.2 | direct_other_host | 0.45 [0.32, 0.59] (25/55) | 0.60 [0.42, 0.76] (21/35) | 0.99 [0.96, 1.00] (268/272) |
| T5 | [GzDf…4ZzG](https://explorer.solana.com/address/GzDfKKoZ6E8ReqKfH96k5bkmA6CEWtnUrQnZjSqW4ZzG) | AgaveBam | 4.3.2, 4.4.0 | shared | 0.47 [0.34, 0.61] (26/55) | 0.61 [0.42, 0.77] (20/33) | 0.94 [0.91, 0.97] (256/272) |
| T6 | [FBKF…jeNi](https://explorer.solana.com/address/FBKFWadXZJahGtFitAsBvbqh5968gLY7dMBBJUoUjeNi) | HarmonicAgave | 4.3.2, 4.4.0 | shared | 0.55 [0.40, 0.68] (29/53) | 0.67 [0.46, 0.83] (18/27) | 0.96 [0.94, 0.98] (270/280) |
| T7 | [5Zqv…q9qk](https://explorer.solana.com/address/5ZqveVffQPiUbkjBg4KD9kib1MKHLqiFno4ke9jSq9qk) | HarmonicAgave | 4.3.2 | shared | 0.56 [0.41, 0.69] (30/54) | 0.59 [0.39, 0.78] (16/27) | 0.93 [0.90, 0.96] (254/272) |
| T8 | [Awes…vpLM](https://explorer.solana.com/address/Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM) | HarmonicFiredancer | 26.10.5 | shared | 0.51 [0.36, 0.66] (24/47) | 0.65 [0.45, 0.81] (20/31) | 0.92 [0.88, 0.95] (249/272) |
| T9 | [6ptu…JwWG](https://explorer.solana.com/address/6ptuwW4rg5A3wP9hNKxeymG3QhSagugPd3grnMTGJwWG) | HarmonicFiredancer | 26.10.5 | shared | 0.58 [0.44, 0.71] (32/55) | 0.74 [0.56, 0.87] (25/34) | 0.98 [0.96, 0.99] (267/272) |
| T10 | [9eGr…8FoY](https://explorer.solana.com/address/9eGrDohdNTAo61DRHyfMuqKWXqYnA3i254Wiszxe8FoY) | Firedancer | 26.10.5 | direct | 0.24 [0.13, 0.38] (13/54) | 0.76 [0.58, 0.89] (25/33) | 0.99 [0.96, 1.00] (268/272) |
| T11 | [BFmq…Wmzy](https://explorer.solana.com/address/BFmqBsaGjeYA7gHpReKmnVkRyhqiMmHoR9ttv1crWmzy) | Firedancer | 26.10.5 | direct | 0.10 [0.03, 0.23] (5/48) | 0.63 [0.42, 0.81] (17/27) | 1.00 [0.98, 1.00] (228/228) |
| T12 | [qZMH…QrbP](https://explorer.solana.com/address/qZMH9GWnnBkx7aM1h98iKSv2Lz5N78nwNSocAxDQrbP) | FireBAM | 26.10.5 | shared | 0.78 [0.40, 0.97] (7/9) | 0.86 [0.42, 1.00] (6/7) | 0.73 [0.63, 0.82] (67/92) |
| T13 | [Ed9W…a2Zv](https://explorer.solana.com/address/Ed9WjPnZfAXsPttcqxMwj94qsuXVRyBsyXnDkxFva2Zv) | Rakurai | 4.4.0 | direct | 0.50 [0.21, 0.79] (6/12) | 0.50 [0.12, 0.88] (3/6) | 1.00 [0.94, 1.00] (64/64) |
| X1 | [9xcX…eczT](https://explorer.solana.com/address/9xcXe4WRAfbVoVgQmFD5xgFHioL9dD7c74pQZ7PeczT) | Raiku | 4.4.0 | direct | 0.70 [0.35, 0.93] (7/10) | 0.71 [0.29, 0.96] (5/7) | 1.00 [0.93, 1.00] (52/52) |
<!-- /table:validators -->

The tests and every threshold were fixed before the campaign in [PREREGISTRATION.md](docs/PREREGISTRATION.md), and its
hash went on chain in a Memo before the first campaign probe. Verdicts per test and family:
[REPORT §3](REPORT.md#3-verdicts).

## How it works

**What did you send?** In one leader turn of one validator, four probes as two pairs: a main pair whose members differ
in fee or cost, and a control pair of identical keys. Each probe is a one-signature transfer of 1 lamport that sets its
compute-unit limit and price.

**Why do the two members write the same accounts?** Each pays 1 lamport to the other member's payer, so the leader
cannot run them in parallel: one has to come first, and the block shows which.

**Why an identical pair in every turn?** Same leader, same moment, same path, nothing to sort by. Its order shows what
breaks ties: arrival, the latest arrival, or nothing related to arrival. That is the baseline every other pair is read
against.

**Why pairs where the keys disagree?** Reward per cost (the key Agave's scheduler computes), CU price and total fee
prefer different members in two of my pair types, so the member that lands first names the key. A third type, where
every key agrees, checks that sorting happens at all.

**Why randomise the send time and the gap?** A transaction that arrives before the turn waits in a buffer; one that
arrives during the turn meets a scheduler that is already building blocks. I drew the send offset per quad from ten
levels between 300 ms before and 900 ms after the first shred of the slot before the turn, and the gap between the
members from 0 to 100 ms, so position and gap are measured factors.

**Why two validators per client?** One validator could be one operator's setup. Two per client family give a first
look at the difference between operators ([REPORT §7](REPORT.md#7-clients-and-operators)).

**What can't this tell apart?** The validator from a block engine in front of it, send time from arrival time below
the network jitter, and a client from its operators' settings, among others: [REPORT §10](REPORT.md#10-what-this-study-does-not-separate).
How the design handles the six points of the brief: [DESIGN.md](docs/DESIGN.md#the-six-things-the-brief-asks-to-account-for).

## Re-run it

```
git clone https://github.com/btccdbcb/inside-the-block && cd inside-the-block
docker compose up                       # offline: recomputes every table, figure and number, prints "0 differences"
docker compose --profile verify up      # online: re-checks 60 random claims, the Memo and ledger = chain on public RPC
```

Or check a block by hand:

<!-- table:examples -->
*Source: results/pairs.csv and results/probes.csv (the first campaign example of each kind, by run and quad).*

Where the two equal-key probes of quad C-T1-001 sit in the block of slot 463,481,852 (needs curl and jq; the public endpoint rate-limits getBlock, so after a 429 wait a few seconds and retry):

```
curl -s https://api.mainnet-beta.solana.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,
  "method":"getBlock","params":[463481852,{"transactionDetails":"signatures",
  "maxSupportedTransactionVersion":0,"rewards":false}]}' \
  | jq '.result.signatures | index("28MtzrFQmAEi7HNGnzPfF52RBxUfbdLiTXeBR2bJEjpdELnCFZJWe5V6i4agNDAgZ6VQtrjRr9LPNAD9XrEVBK4a"),
                             index("5r7d2mYcjWWFm44W15cennWbjRivQkEvBXKyRUG71Cmtmpauwj8cpiiY1PJ6AQWUrUxTVDLMtu9GsWow3PYriP7v")'
```

It prints 89 and 114: the probe sent first is earlier in the block.

| example | quad | slot | probes, in sending order | position in block |
|---|---|---|---|---|
| equal keys (A = A) | C-T1-001 | 463,481,852 | [A sent first](https://explorer.solana.com/tx/28MtzrFQmAEi7HNGnzPfF52RBxUfbdLiTXeBR2bJEjpdELnCFZJWe5V6i4agNDAgZ6VQtrjRr9LPNAD9XrEVBK4a), [A sent second](https://explorer.solana.com/tx/5r7d2mYcjWWFm44W15cennWbjRivQkEvBXKyRUG71Cmtmpauwj8cpiiY1PJ6AQWUrUxTVDLMtu9GsWow3PYriP7v) | 89, 114 |
| sent before the turn, HI = higher fee and priority | C-T1-001 | 463,481,852 | [LO sent first](https://explorer.solana.com/tx/KYRLSuipzQgmMuLFu8y8mKpRRSo83CJ4tiZvWhxVKvnNEjdDJd2MnNnWY67yMo4XTdunRb3TRJZLbx5LiJsqXDH), [HI sent second](https://explorer.solana.com/tx/PZZyKVrEbcPh8AMn9gMJJmv9F4M9yfrtMevPhxcyjNcwJWVt3qh8Z3Nnf3RHm3fwGMSEvSqTTSeMEb2Y1EwgFkx) | 88, 113 |
| keys disagree: Q = higher reward per cost, P = higher CU price | C-T10-007 | 463,499,513 | [Q sent first](https://explorer.solana.com/tx/4FPVrYEAwQH1J3H7RvLx6C2RmAspnT4ys36DrixkvHbo492b8jm6L8wLRDo6KRyeJmzQAKuMw27fBNgHerkzigE2), [P sent second](https://explorer.solana.com/tx/4ULKm5zE2sHHQv4Mbnmf986wYg8tiCHRxShFKeaZxzfK6B9iyLUbBDWsdDkdCS7DWoY6rgCv7AyZZ9UbZei5j9rT) | 113, 134 |
| landed with the leader before the target | C-T5-011 | 463,520,234 | [A](https://explorer.solana.com/tx/5eeefPTiwaqsQwHrk1nF6v1MMW6RQBgzsoQd7ywPPkXLBtWatZftAV1QmAQimciPt6AXXyRKXsYMB8CUfq71ZLxa) | 11 |
<!-- /table:examples -->

The raw data are in `data/raw/` (one directory per run, [DATA.md](docs/DATA.md)); what the verifier checks is in
[VERIFY.md](docs/VERIFY.md).

## Money and keys

<!-- table:money -->
*Source: data/live/ledger.csv, data/live/ledger_check.json.*

|  |  |
|---|---|
| fees paid | 15,463,152 lamports (0.015463 SOL) of the 30,000,000 cap |
| ledger = chain | not checked yet |
| sweep | not yet |
<!-- /table:money -->

The fee cap is compiled into the sender: 30,000,000 lamports (0.03 SOL), split into sub-budgets, with every probe's
worst-case fee reserved in a ledger before its bytes can be sent. Everything is a dry run unless `itb live --live` is
given. Keys are read from a path outside the repository; a pre-commit check and CI refuse any committed keypair, and
gitleaks scans every commit.

## Files

```
crates/itb/          the sender (Rust): timing, QUIC, probes, ledger, live programme
analysis/            collection, outcomes, the pre-registered tests, verification, figures, report tables (Python)
design/              the frozen targets, design rows and campaign file
data/                raw runs, dry runs and the live ledger
results/, figures/   everything derived, regenerated by `docker compose up`
docs/                PREREGISTRATION, DESIGN, DATA, POWER, VERIFY
REPORT.md            the full results, what went wrong, what it does not separate, cost
```

Apache License 2.0 ([LICENSE](LICENSE)).
