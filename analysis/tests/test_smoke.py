import pytest

from itb_analysis import cli


def test_cli_requires_a_command():
    with pytest.raises(SystemExit):
        cli.main([])


def test_cli_design(tmp_path):
    import json
    from pathlib import Path

    targets = Path(__file__).resolve().parents[2] / "design" / "targets.json"
    assert cli.main(["design", "--dir", str(tmp_path), "--targets", str(targets), "--seed", "7"]) == 0
    camp = json.loads((tmp_path / "campaign.json").read_text())
    assert camp["seed"] == 7 and camp["rows"] > 1000
