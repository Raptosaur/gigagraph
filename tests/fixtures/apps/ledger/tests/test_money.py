import pytest

from ledger.money import Money, total


def test_addition_sums_cents():
    assert (Money(100) + Money(250)).cents == 350


def test_addition_rejects_currency_mismatch():
    with pytest.raises(ValueError):
        Money(1, "USD") + Money(1, "EUR")


@pytest.mark.parametrize("text,cents", [("1.00", 100), ("0.05", 5)])
def test_parse_reads_decimals(text, cents):
    assert Money.parse(text).cents == cents


def test_total_of_empty_is_zero():
    assert total([]).is_zero


class TestMoneyProperties:
    def test_zero_is_zero(self):
        assert Money.zero().is_zero

    def test_negation_flips_sign(self):
        assert (-Money(5)).cents == -5

    def helper_amount(self):
        return Money(7)
