import { findBook } from "./catalog";

type Line = { isbn: string; quantity: number; priceCents: number };

export class Cart {
  private lines: Line[] = [];

  add(isbn: string, quantity: number, priceCents: number): void {
    const existing = this.lines.find((l) => l.isbn === isbn);
    if (existing) {
      existing.quantity += quantity;
      return;
    }
    this.lines.push({ isbn, quantity, priceCents });
  }

  remove(isbn: string): boolean {
    const before = this.lines.length;
    this.lines = this.lines.filter((l) => l.isbn !== isbn);
    return this.lines.length < before;
  }

  get total(): number {
    return this.lines.reduce((sum, l) => sum + l.priceCents * l.quantity, 0);
  }

  static empty(): Cart {
    return new Cart();
  }
}

export async function addToCart(cart: Cart, isbn: string, quantity = 1): Promise<boolean> {
  const book = await findBook(isbn);
  if (!book) return false;
  cart.add(isbn, quantity, 0);
  return true;
}
