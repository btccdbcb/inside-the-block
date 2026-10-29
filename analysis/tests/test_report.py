from itb_analysis import report


def test_render_replaces_only_table_blocks():
    ctx = {"results": None, "explore": None, "timing": None, "verify": {"checked": 3, "failures": [], "pass": True},
           "power": None, "raw": [], "dry": [], "ledger": [
               {"sub_budget": "pilot", "kind": "probe", "status": "landed", "fee_lamports": "5096"},
               {"sub_budget": "pilot", "kind": "probe", "status": "expired", "fee_lamports": ""}]}
    doc = "Text before.\n<!-- table:cost -->\nold\n<!-- /table:cost -->\nmiddle\n<!-- table:verify -->\n" \
          "<!-- /table:verify -->\nafter\n"
    out = report.render_doc(doc, ctx)
    assert out.startswith("Text before.\n<!-- table:cost -->\n*Source:")
    assert "old" not in out and "\nmiddle\n" in out and out.endswith("after\n")
    assert "| pilot | probe | 2 | 1 | 5,096 |" in out
    assert "| claims checked offline | 3 |" in out
    assert report.render_doc(out, ctx) == out  # stable


def test_unknown_table_is_an_error():
    try:
        report.render_doc("<!-- table:nope -->\n<!-- /table:nope -->", {})
    except KeyError:
        return
    raise AssertionError("unknown table accepted")


def test_inline_values():
    ctx = {"results": {"P1": {"JitoLabs": {"k": 53, "n": 69, "p": 53 / 69, "ci95": [0.65, 0.86], "p_value": 0.0004}}},
           "n": {"fees_total": 1197532}}
    doc = ("F = <!-- v:results.P1.JitoLabs|prop -->x<!-- /v -->, p <!-- v:results.P1.JitoLabs.p_value|p -->"
           "<!-- /v -->, fees <!-- v:n.fees_total|int -->?<!-- /v --> lamports")
    out = report.render_doc(doc, ctx)
    assert "-->0.77 [0.65, 0.86] (53/69)<!--" in out and "-->< 0.001<!--" in out and "-->1,197,532<!--" in out
    try:
        report.render_doc("<!-- v:results.P9 --><!-- /v -->", ctx)
    except KeyError:
        return
    raise AssertionError("missing value accepted")


def test_memo_ledger_money_tables():
    h = "ab" * 32
    sig = "5" * 88
    ctx = {"campaign": {"cap_lamports": 30_000_000},
           "memo": {"signature": sig, "slot": 470_000_000, "block_time": 1_793_000_000,
                    "text": f"inside-the-block prereg design.jsonl={h} campaign.json={h} PREREGISTRATION.md=x "
                            f"pilot_decision={h}",
                    "expected": {"design.jsonl": h, "campaign.json": h, "PREREGISTRATION.md": h, "pilot_decision": h},
                    "first_campaign_probe": {"first_send_ms": 1_793_100_000_000, "quad_id": "A-T1-001",
                                             "min_slot0": 470_100_000},
                    "code_changed_since_freeze": [], "problems": ["PREREGISTRATION.md: memo 'x'"], "pass": False},
           "ledger_check": {"totals": {"ledger": 1000, "chain": 1000, "balances": 1000}, "start_balance": 5000,
                            "end_balance": 0, "left_to_others": 4000, "arrived_from_others": 0, "accounts": 9,
                            "transactions": 120, "mismatches": [], "unrecorded": [], "balance_gaps": [],
                            "unavailable": [], "pass": True},
           "ledger": [{"kind": "probe", "status": "landed", "fee_lamports": "900", "signature": "a"},
                      {"kind": "sweep", "status": "landed", "fee_lamports": "100", "signature": sig}]}
    memo = report.t_memo(ctx)
    assert f"[555555…5555](https://explorer.solana.com/tx/{sig}), slot 470,000,000, 2026-10-26" in memo
    assert "| sha256 of PREREGISTRATION.md | `abababababababab…` ≠ the Memo |" in memo
    assert "| sha256 of design.jsonl | `abababababababab…` = the Memo |" in memo
    assert "| first campaign turn | slot 470,100,000 |" in memo and "| result | FAIL: PREREGISTRATION.md" in memo
    lc = report.t_ledger_check(ctx)
    assert "| chain: `meta.fee` of every transaction paid by my accounts | 1,000 |" in lc
    assert "| mismatches, unrecorded, balance gaps, not returned | 0 · 0 · 0 · 0 |" in lc and "LEDGER = CHAIN" in lc
    money = report.t_money(ctx)
    assert "| fees paid | 1,000 lamports (0.000001 SOL) of the 30,000,000 cap |" in money
    assert "| ledger = chain | yes, to the lamport |" in money and "| sweep | 1 transaction; the last [5" in money
    empty = {"campaign": None, "memo": None, "ledger_check": None, "ledger": []}
    assert "no data" in report.t_memo(empty) + report.t_ledger_check(empty) + report.t_money(empty)
