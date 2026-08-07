"""Small helpers shared across the service layer."""

import json
import re


def normalize_email(email):
    return email.strip().lower()


def load_settings(path):
    with open(path) as fh:
        return json.load(fh)


def slugify(text: str) -> str:
    cleaned = re.sub(r"[^a-z0-9]+", "-", text.lower())
    return cleaned.strip("-")
