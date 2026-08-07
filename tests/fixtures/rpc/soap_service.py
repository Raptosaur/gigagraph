from spyne import Application, Integer, ServiceBase, Unicode, rpc


class UserService(ServiceBase):
    @rpc(Integer, _returns=Unicode)
    def get_user(ctx, uid):
        return "user-%d" % uid

    @rpc(_returns=Unicode)
    def list_users(ctx):
        return "all"
