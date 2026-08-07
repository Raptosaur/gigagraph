from service import load_widget


def make_fixture_id():
    return 7


def test_load_widget():
    assert load_widget(make_fixture_id())["id"] == 7
