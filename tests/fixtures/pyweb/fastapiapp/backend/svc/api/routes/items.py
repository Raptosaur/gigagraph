from fastapi import APIRouter

router = APIRouter(prefix="/items", tags=["items"])


@router.get("/")
def read_items():
    return []


@router.put("/{id}")
def update_item(id: int):
    return {}


@router.delete("/{id}")
def delete_item(id: int):
    return {}
