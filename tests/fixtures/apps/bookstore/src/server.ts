import express from "express";
import { findBook, searchBooks } from "./catalog";
import { Cart, addToCart } from "./cart";

const app = express();
const carts = new Map<string, Cart>();

app.get("/api/books/:isbn", async (req, res) => {
  const book = await findBook(req.params.isbn);
  res.json(book ?? {});
});

app.get("/api/books", async (req, res) => {
  res.json(await searchBooks(String(req.query.term ?? "")));
});

app.post("/api/carts/:id/lines", async (req, res) => {
  const cart = carts.get(req.params.id) ?? Cart.empty();
  const ok = await addToCart(cart, req.body.isbn, req.body.quantity);
  carts.set(req.params.id, cart);
  res.status(ok ? 201 : 404).end();
});

export function start(port: number) {
  return app.listen(port, () => console.log(`listening on ${port}`));
}

export { app };
