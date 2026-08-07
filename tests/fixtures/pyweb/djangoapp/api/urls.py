from django.urls import include, path

from api.apis import FileStartApi

urlpatterns = [
    path("users/", include(("users.urls", "users"))),
    path(
        "upload/",
        include(
            (
                [
                    path("start/", FileStartApi.as_view(), name="start"),
                ],
                "upload",
            )
        ),
    ),
]
