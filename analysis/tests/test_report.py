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
