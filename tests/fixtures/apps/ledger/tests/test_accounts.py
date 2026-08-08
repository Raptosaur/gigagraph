import unittest

from ledger.accounts import Journal, chart_of_accounts, is_balanced
from ledger.money import Money


def test_transfer_balances_out(journal):
    entries = journal.account("cash").entries + journal.account("revenue").entries
    assert is_balanced(entries)


def test_chart_of_accounts_filters_by_kind():
    assert chart_of_accounts("asset") == ("cash",)


class JournalTestCase(unittest.TestCase):
    def setUp(self):
        self.journal = Journal()

    def test_unknown_account_is_created(self):
        self.assertEqual(self.journal.account("rent").balance().cents, 0)

    def test_balances_lists_every_account(self):
        self.journal.transfer("cash", "rent", Money(10))
        self.assertEqual(len(self.journal.balances()), 2)

    def tearDown(self):
        self.journal = None
