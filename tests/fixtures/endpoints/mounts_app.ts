import express, { Router } from "express";
import shopRoutes from "./mounts_routes";

const app = express();
const admin = Router();

admin.get("/settings", (req: any, res: any) => res.end());

app.get("/status", (req: any, res: any) => res.end());

app.use("/shop", shopRoutes);
app.use("/admin", admin);
