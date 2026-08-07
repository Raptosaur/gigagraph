from django.urls import path, re_path
from django.conf.urls import url
from . import views


def report_index(request):
    return None


urlpatterns = [
    path("reports/<int:pk>/", views.report_detail),
    path("reports/", report_index),
    url(r"^archive/$", views.archive),
    re_path(r"^articles/(?P<year>[0-9]{4})/$", views.year_archive),
]
