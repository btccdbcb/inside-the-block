from itb_analysis import cli


def test_cli_runs():
    assert cli.main([]) == 0
