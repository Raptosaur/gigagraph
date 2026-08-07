const Fastify = require('fastify');
const userRoutes = require('./users.routes');

const app = Fastify();
app.register(userRoutes, { prefix: '/v2' });

module.exports = app;
