import json

from pysvc.helpers import slugify


def publish(title, body):
    slug = slugify(title)
    record = {"slug": slug, "body": body}
    return json.dumps(record)
