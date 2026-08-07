from fastapi import FastAPI

from svc.api.main import api_router
from svc.core.config import settings

app = FastAPI(title="svc")
app.include_router(api_router, prefix=settings.API_V1_STR)
