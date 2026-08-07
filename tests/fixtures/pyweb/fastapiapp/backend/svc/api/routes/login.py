from fastapi import APIRouter

router = APIRouter(tags=["login"])


@router.post("/login/access-token")
def login_access_token():
    return {}
