import { describe, it, expect, beforeEach } from "vitest";

import { CatalogService, toView } from "../src/catalog";
import { formatPrice, slugify } from "../src/format";

describe("catalog", () => {
  beforeEach(() => {
    resetFixtures();
  });

  it("renders a book view", () => {
    const view = toView({ isbn: "1", title: "Dune", priceCents: 1250 });
    expect(view.price).toBe("$12.50");
  });

  it("slugifies titles", () => {
    expect(slugify("The Left Hand of Darkness")).toBe("the-left-hand-of-darkness");
  });

  it.each([100, 250])("formats %i cents", (cents) => {
    expect(formatPrice(cents)).toMatch(/^\$/);
  });

  describe("CatalogService", () => {
    it("exposes its ttl", () => {
      expect(new CatalogService(500).ttl()).toBe(500);
    });

    it.skip("warms the cache", async () => {
      expect(await new CatalogService(1).warm()).toBeGreaterThan(0);
    });
  });
});

describe("format", () => {
  it("formats whole dollars", () => {
    expect(formatPrice(100)).toBe("$1.00");
  });
});

function resetFixtures(): boolean {
  return true;
}
