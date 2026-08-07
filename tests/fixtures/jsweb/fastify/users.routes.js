// Fastify plugin mounted with a prefix by app.js. Also carries the options
// object route form (`url` key, not `path`).
const fp = require('fastify-plugin');

async function routes(fastify, opts) {
  fastify.get('/profile', async () => ({}));
  fastify.route({ method: 'PUT', url: '/settings', handler: async () => ({}) });
}

module.exports = routes;
