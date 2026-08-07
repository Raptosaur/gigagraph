from flask import Flask
from flask.views import MethodView

app = Flask(__name__)


class CounterAPI(MethodView):
    def get(self):
        return "0"

    def post(self):
        return "1"


app.add_url_rule("/counter", view_func=CounterAPI.as_view("counter"))
