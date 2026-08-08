from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass(frozen=True)
class Money:
    cents: int
    currency: str = "USD"

    def __add__(self, other: "Money") -> "Money":
        _assert_same_currency(self, other)
        return Money(self.cents + other.cents, self.currency)

    def __neg__(self) -> "Money":
        return Money(-self.cents, self.currency)

    def as_decimal(self) -> Decimal:
        return Decimal(self.cents) / 100

    @classmethod
    def zero(cls, currency: str = "USD") -> "Money":
        return cls(0, currency)

    @staticmethod
    def parse(text: str) -> "Money":
        amount, _, currency = text.partition(" ")
        return Money(int(Decimal(amount) * 100), currency or "USD")

    @property
    def is_zero(self) -> bool:
        return self.cents == 0


def _assert_same_currency(a: Money, b: Money) -> None:
    if a.currency != b.currency:
        raise ValueError(f"currency mismatch: {a.currency} != {b.currency}")


def total(amounts: list[Money]) -> Money:
    acc = Money.zero()
    for amount in amounts:
        acc = acc + amount
    return acc
