const { rebuild } = require("./indexer");

async function handler(event) {
  const count = await rebuild("{}");
  return { statusCode: 200, body: String(count) };
}

module.exports = { handler };
