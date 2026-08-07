import { Hono } from "hono";

const app = new Hono();

app.get("/hedgehogs", (c: any) => c.json([]));
