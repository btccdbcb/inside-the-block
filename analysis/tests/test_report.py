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
