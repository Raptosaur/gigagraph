const fs = require("fs");
const { join } = require("path");

function readJson(file) {
  const text = fs.readFileSync(join(__dirname, file), "utf-8");
  return JSON.parse(text);
}

const double = (x) => x * 2;

const helpers = {
  triple(x) {
    return x * 3;
  },
  quadruple: function (x) {
    return double(double(x));
  },
};

class Counter {
  #count = 0;
  increment() {
    this.#count += 1;
    return this.#count;
  }
  reset = () => {
    this.#count = 0;
  };
}

Counter.prototype.decrement = function () {
  this._count -= 1;
};

module.exports = { readJson, double, helpers, Counter };
