// Fastify plugin mounted with a prefix by app.js. Also carries the options
// object route form (`url` key, not `path`) — single-method string and
// multi-method array (`method: ['GET', 'POST']`) variants.
const fp = require('fastify-plugin');

async function routes(fastify, opts) {
  fastify.get('/profile', async () => ({}));
  fastify.route({ method: 'PUT', url: '/settings', handler: async () => ({}) });
  fastify.route({
    method: ['GET', 'POST'],
    url: '/bulk',
    handler: async () => ({}),
  });
}

module.exports = routes;
