import requests


def fetch_item(item_id):
    return requests.get("/items/9")


def submit_item(payload):
    return requests.post("/items", json=payload)
