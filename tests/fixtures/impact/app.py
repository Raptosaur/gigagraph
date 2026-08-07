from flask import Flask

from service import load_widget

app = Flask(__name__)


@app.route("/widgets/<int:widget_id>")
def get_widget(widget_id):
    return load_widget(widget_id)
