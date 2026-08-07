import { Router } from "express";

const router = Router();

export function listSkus(req: any, res: any): void {
  res.json([]);
}

router.get("/skus/:id", listSkus);

export default router;
