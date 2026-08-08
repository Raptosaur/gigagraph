from fastapi import APIRouter, FastAPI, HTTPException

from .accounts import Journal, is_balanced
from .money import Money

app = FastAPI(title="ledger")
router = APIRouter(prefix="/v1")
journal = Journal()


@app.get("/health")
async def health() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/accounts/{name}/balance")
async def read_balance(name: str) -> dict[str, int]:
    account = journal.accounts.get(name)
    if account is None:
        raise HTTPException(status_code=404, detail="no such account")
    return {"cents": account.balance().cents}


@router.post("/transfers")
async def create_transfer(debit: str, credit: str, amount: str) -> dict[str, bool]:
    money = Money.parse(amount)
    journal.transfer(debit, credit, money)
    return {"balanced": is_balanced(journal.account(debit).entries + journal.account(credit).entries)}


@router.get("/balances")
def list_balances() -> dict[str, int]:
    return {name: m.cents for name, m in journal.balances().items()}


app.include_router(router)
