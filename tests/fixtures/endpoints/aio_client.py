import aiohttp


async def poll_items(session):
    return await session.get("/items/3")
