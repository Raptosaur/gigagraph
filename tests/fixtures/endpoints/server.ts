import express from "express";

const app = express();

export function getUser(req: any, res: any): void {
  res.json({ id: req.params.id });
}

app.get("/users/:id", getUser);
app.post("/users", (req: any, res: any) => {
  res.status(201).end();
});
