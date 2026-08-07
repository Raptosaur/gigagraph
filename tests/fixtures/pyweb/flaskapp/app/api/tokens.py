from app.api import bp


@bp.route("/tokens", methods=["POST"])
def create_token():
    return {}


@bp.route("/tokens", methods=["DELETE"])
def revoke_token():
    return ""
