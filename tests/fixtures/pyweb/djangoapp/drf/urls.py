from django.urls import include, path
from rest_framework import routers

from drf.apis import ArticleViewSet, FeedViewSet, StatsViewSet

router = routers.DefaultRouter()
router.register(r"articles", ArticleViewSet)
router.register("feeds", FeedViewSet)
router.register("stats", StatsViewSet, basename="stats")

urlpatterns = [
    path("v1/", include((router.urls, "api"))),
]
