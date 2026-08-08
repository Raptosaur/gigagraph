import functools
from typing import Iterable

from .money import Money, total


class Account:
    def __init__(self, name: str, kind: str = "asset") -> None:
        self.name = name
        self.kind = kind
        self.entries: list[Money] = []

    def post(self, amount: Money) -> None:
        self.entries.append(amount)

    def balance(self) -> Money:
        return total(self.entries)

    def __repr__(self) -> str:
        return f"<Account {self.name} {self.balance().cents}>"


class Journal:
    def __init__(self) -> None:
        self.accounts: dict[str, Account] = {}

    def account(self, name: str) -> Account:
        if name not in self.accounts:
            self.accounts[name] = Account(name)
        return self.accounts[name]

    def transfer(self, debit: str, credit: str, amount: Money) -> None:
        self.account(debit).post(amount)
        self.account(credit).post(-amount)

    def balances(self) -> dict[str, Money]:
        return {name: acct.balance() for name, acct in self.accounts.items()}

    async def snapshot(self) -> dict[str, int]:
        return {name: m.cents for name, m in self.balances().items()}


@functools.lru_cache(maxsize=8)
def chart_of_accounts(kind: str) -> tuple[str, ...]:
    return tuple(sorted(name for name in _DEFAULTS if _DEFAULTS[name] == kind))


def is_balanced(amounts: Iterable[Money]) -> bool:
    return total(list(amounts)).is_zero


_DEFAULTS = {"cash": "asset", "revenue": "income", "rent": "expense"}
