// Koa router constructed with a prefix option (koa-router ctor args are
// captured now, and assigned_to binds the variable).
const Router = require('@koa/router');

const router = new Router({ prefix: '/kapi' });

router.get('/pets', listPets);

function listPets(ctx) {
  ctx.body = [];
}

module.exports = router;
