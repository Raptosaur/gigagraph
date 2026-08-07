def setup_routes(router, handlers):
    router.add_get("/items", handlers.list_items)
    router.add_post("/items", handlers.create_item)
