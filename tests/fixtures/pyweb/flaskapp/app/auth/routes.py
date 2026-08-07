from flask import render_template

from app.auth import bp


@bp.route("/login", methods=["GET", "POST"])
def login():
    return render_template("login.html")


@bp.get("/logout")
def logout():
    return ""
