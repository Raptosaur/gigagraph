from flask import Blueprint

bp = Blueprint("api", __name__, url_prefix="/v1")

from app.api import tokens  # noqa
