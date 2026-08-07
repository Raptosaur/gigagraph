import requests


def fetch_widget(widget_id):
    return requests.get("http://api.internal/widgets/%d" % widget_id)
