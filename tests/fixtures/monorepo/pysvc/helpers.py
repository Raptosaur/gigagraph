def slugify(title):
    return title.lower().replace(" ", "-")


def truncate(text, limit=80):
    return text[:limit]
