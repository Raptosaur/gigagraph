from rest_framework.views import APIView


class UserListApi(APIView):
    def get(self, request):
        return None

    def post(self, request):
        return None


class UserDetailApi(APIView):
    def get(self, request, user_id):
        return None
