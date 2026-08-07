import Router from "@koa/router";

const router = new Router();

export function koalaShow(ctx: any): void {
  ctx.body = { id: ctx.params.id };
}

router.get("/koalas/:id", koalaShow);
