const { Cart } = require("../src/cart");

describe("Cart", () => {
  let cart;

  beforeEach(() => {
    cart = Cart.empty();
  });

  test("sums line totals", () => {
    cart.add("1", 2, 500);
    expect(cart.total).toBe(1000);
  });

  test("removes a line", () => {
    cart.add("1", 1, 100);
    expect(cart.remove("1")).toBe(true);
  });

  test.skip("handles currency conversion", () => {
    expect(true).toBe(false);
  });

  it("merges duplicate isbns", () => {
    cart.add("1", 1, 100);
    cart.add("1", 3, 100);
    expect(cart.total).toBe(400);
  });
});
