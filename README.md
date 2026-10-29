# Inside the Block

What a Solana leader does with two of my transactions that write the same accounts, depending on its client, on when in
the hand-over they arrive, on the gap between them and on how full the block is. Measured on mainnet with my own probes,
sent straight to each leader's TPU port at randomised moments of its turn.

<!-- TODO(data): three sentences, in this order: (1) equal-key order F per family (k/n, 95 % CI); (2) how landing and
leakage to the preceding leader change with the send offset; (3) the load result. Then one scope line: validators,
client families, probes, mainnet dates (UTC), fees spent. -->

## Validators measured

<!-- table:validators -->
*Source: gossip views in data/raw/*/quads.jsonl; results/results.json → descriptive.per_target.*

| slot | validator | client (gossip) | versions seen | ingress | F, equal keys | R: higher key first | probes in its turn |
|---|---|---|---|---|---|---|---|
| no data |  |  |  |  |  |  |  |
<!-- /table:validators -->

The tests and every threshold were fixed before the campaign in [PREREGISTRATION.md](docs/PREREGISTRATION.md), and its
hash went on chain in a Memo before the first campaign probe. Verdicts per test and family:
[REPORT §3](REPORT.md#3-verdicts).

## The turn

<!-- TODO(data): <picture> of figures/turn-map-{light,dark}.svg with one caption line. -->

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

**How do I know the analysis wasn't tuned to the data?** The rules, the design rows and the hashes of the three
analysis files were frozen and their hashes written to mainnet before the first campaign probe.

**What can't this tell apart?** The validator from a block engine in front of it, send time from arrival time below
the network jitter, and a client from its operators' settings, among others: [REPORT §10](REPORT.md#10-what-this-study-does-not-separate).
How the design handles the six points of the brief: [DESIGN.md](docs/DESIGN.md#the-six-things-the-brief-asks-to-account-for).

## Re-run it

```
git clone https://github.com/btccdbcb/inside-the-block && cd inside-the-block
docker compose up                       # offline: recomputes every table, figure and number, prints "0 differences"
docker compose --profile verify up      # online: re-fetches 60 random claims from public mainnet RPC and checks them
```

<!-- TODO(data): one curl getBlock example for a landed probe and three explorer links (an equal-key pair, a pre-turn
pair, a probe that landed with the preceding leader). -->

The raw data are in `data/raw/` (one directory per run, [DATA.md](docs/DATA.md)); what the verifier checks is in
[VERIFY.md](docs/VERIFY.md).

## Money and keys

<!-- TODO(data): fees spent vs the cap; ledger = chain to the lamport; the sweep transaction. -->

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
