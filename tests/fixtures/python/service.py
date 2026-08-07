"""User service fixture exercising the Python extraction query."""

import json
import os.path
import logging as log
from collections import OrderedDict
from util import normalize_email, load_settings as load
from . import repo
from ..core import events

MAX_ATTEMPTS = 3


def make_service(root):
    settings = load(os.path.join(root, "settings.json"))
    return UserService(settings)


async def warm_cache(service):
    await service.refresh()


class UserService:
    def __init__(self, settings):
        self.settings = settings
        self.users = OrderedDict()

    def register(self, email, name):
        normalized = normalize_email(email)
        if not self.validate(normalized):
            log.error("bad email %s", normalized)
            return None
        user = self.build_user(normalized, name)
        self.users[normalized] = user
        events.emit("registered", user)
        return user

    def validate(self, email):
        return "@" in email

    def build_user(self, email, name):
        return {"email": email, "name": name}

    @staticmethod
    def merge(base, extra=None):
        merged = dict(base)
        merged.update(extra or {})
        return merged

    @property
    def count(self):
        return len(self.users)

    async def refresh(self):
        attempts = 0
        for key in self.users:
            if self.validate(key):
                attempts += 1
        while attempts < MAX_ATTEMPTS:
            attempts += 1
        return json.dumps(list(self.users))

    def export(self):
        def render(user):
            return "%s <%s>" % (user["name"], user["email"])

        return [render(u) for u in self.users.values()]


class AdminService(UserService):
    def promote(self, email):
        user = super().build_user(email, "admin")
        self.users[email] = user
        return user


make_service(".")
