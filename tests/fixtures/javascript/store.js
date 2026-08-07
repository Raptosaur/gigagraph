// Untyped DI shapes: construction is the only type evidence in JS.
class Store extends BaseStore {
  constructor() {
    super();
    this.db = new Database();
  }

  run(sql) {
    const q = new QueryBuilder();
    return q.exec(sql);
  }
}

class RemoteStore extends backends.HttpStore {}

module.exports = { Store, RemoteStore };
