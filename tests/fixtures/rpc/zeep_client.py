import zeep


def fetch_user(uid):
    client = zeep.Client("http://users.example.com/service?wsdl")
    return client.service.get_user(uid)
