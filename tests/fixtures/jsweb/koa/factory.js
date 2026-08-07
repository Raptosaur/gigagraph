// koajs/examples blog shape: the require('@koa/router')() factory idiom (the
// import must still register as evidence) and verb-on-verb chains.
const router = require('@koa/router')();

router.get('/factory-route', show).post('/factory-post', createIt);

function show(ctx) {}

function createIt(ctx) {}

module.exports = router;
