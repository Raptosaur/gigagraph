from fastapi import APIRouter

from svc.api.routes import items, login

api_router = APIRouter()
api_router.include_router(items.router)
api_router.include_router(login.router)
