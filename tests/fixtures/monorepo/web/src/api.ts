import express from "express";
import { queryRows } from "./db";

export function startApi(port: number): void {
  const app = express();
  app.get("/users", listUsers);
  app.listen(port);
}

async function listUsers(req: unknown, res: { json(v: unknown): void }): Promise<void> {
  const rows = await queryRows("select * from users", []);
  res.json(rows);
}

export function sumOdds(values: number[]): number {
  let acc = 0;
  for (const v of values) {
    if (v % 2 === 1) {
      acc += v;
    }
  }
  return acc;
}
