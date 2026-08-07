from rest_framework import decorators, viewsets


class ArticleViewSet(viewsets.ModelViewSet):
    def list(self, request):
        return None

    @decorators.action(detail=True, methods=["POST"])
    def publish(self, request, pk=None):
        return None

    @decorators.action(detail=False, url_path="export/(?P<fmt>[^/.]+)", methods=["GET"])
    def export(self, request, fmt=None):
        return None


class FeedViewSet(viewsets.ReadOnlyModelViewSet):
    pass


class StatsViewSet(viewsets.GenericViewSet):
    def get_queryset(self):
        return None

    @decorators.action(detail=False, methods=["GET"])
    def summary(self, request):
        return None
