from flask import Flask, jsonify

from pysvc.helpers import truncate

app = Flask(__name__)


@app.route("/items")
def list_items():
    preview = truncate("catalog of things")
    return jsonify({"preview": preview})
