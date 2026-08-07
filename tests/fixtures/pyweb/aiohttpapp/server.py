from aiohttp import web


async def index(request):
    return web.Response()


async def vote(request):
    return web.Response()


async def ws_handler(request):
    return web.Response()


async def gql(request):
    return web.Response()


routes = web.RouteTableDef()


@routes.get("/table")
async def table(request):
    return web.Response()


def create_app():
    app = web.Application()
    app.router.add_get("/", index)
    app.router.add_post("/vote/{id}", vote)
    app.add_routes([web.get("/ws", ws_handler)])
    app.router.add_static("/static/", path="static")
    add_route = app.router.add_route
    add_route("GET", "/graphql", gql)
    app.add_routes(routes)
    return app
