// fastify/demo shape: @fastify/autoload derives the /tasks prefix from the
// directory path (package.json carries the plugin; no explicit register).
const { fastify } = require('fastify');

module.exports = async function (fastify, opts) {
  fastify.get('/', async () => []);
  fastify.post('/run', async () => ({}));
};
