from django.conf.urls import url
from django.urls import path

from users.apis import UserDetailApi, UserListApi


def export_users(request):
    return None


urlpatterns = [
    path("", UserListApi.as_view(), name="list"),
    path("<int:user_id>/", UserDetailApi.as_view(), name="detail"),
    url(r"^export/$", export_users),
]
