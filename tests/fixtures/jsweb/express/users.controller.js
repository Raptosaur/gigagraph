function list(req, res) {
  res.json([]);
}

function create(req, res) {
  res.status(201).end();
}

function remove(req, res) {
  res.status(204).end();
}

module.exports = { list, create, remove };
