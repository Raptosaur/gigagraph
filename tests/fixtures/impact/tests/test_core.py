from core import parse_widget


def test_parse():
    assert parse_widget("3")["id"] == 3
