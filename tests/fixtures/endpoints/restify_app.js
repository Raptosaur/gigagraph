const restify = require("restify");

const server = restify.createServer();

function listRhinos(req, res, next) {
  res.send([]);
  next();
}

server.get("/rhinos", listRhinos);
