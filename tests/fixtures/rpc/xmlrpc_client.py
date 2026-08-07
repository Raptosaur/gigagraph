import xmlrpc.client


def compute():
    proxy = xmlrpc.client.ServerProxy("http://localhost:8000/")
    return proxy.multiply(3, 4)
