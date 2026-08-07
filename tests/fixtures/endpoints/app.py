from flask import Flask

app = Flask(__name__)


@app.route("/items", methods=["POST"])
def create_item():
    return ""


@app.route("/items/<int:item_id>")
def get_item(item_id):
    return ""
