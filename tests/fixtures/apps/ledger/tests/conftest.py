import pytest

from ledger.accounts import Journal
from ledger.money import Money


@pytest.fixture
def journal() -> Journal:
    j = Journal()
    j.transfer("cash", "revenue", Money(1000))
    return j


@pytest.fixture(scope="session")
def usd() -> Money:
    return Money.zero("USD")
