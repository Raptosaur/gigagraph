const { rebuild } = require("./indexer");

exports.handler = async (event) => {
  const count = await rebuild(event.body ?? "{}");
  return { statusCode: 202, body: JSON.stringify({ count }) };
};

exports.warm = function warm() {
  return rebuild("{}");
};
