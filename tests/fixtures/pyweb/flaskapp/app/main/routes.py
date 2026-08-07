from flask import render_template

from app.main import bp


@bp.route("/")
def index():
    return render_template("index.html")


@bp.route("/user/<username>")
def profile(username):
    return render_template("user.html")
