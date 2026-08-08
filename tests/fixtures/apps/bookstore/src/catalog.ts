import { PrismaClient } from "@prisma/client";
import { formatPrice } from "./format";

const prisma = new PrismaClient();

export interface BookView {
  isbn: string;
  title: string;
  price: string;
}

export async function findBook(isbn: string): Promise<BookView | null> {
  const row = await prisma.book.findUnique({ where: { isbn } });
  if (!row) return null;
  return toView(row);
}

export async function searchBooks(term: string, limit = 20): Promise<BookView[]> {
  const rows = await prisma.book.findMany({
    where: { title: { contains: term } },
    take: limit,
  });
  return rows.map(toView);
}

export function toView(row: { isbn: string; title: string; priceCents: number }): BookView {
  return { isbn: row.isbn, title: row.title, price: formatPrice(row.priceCents) };
}

export class CatalogService {
  constructor(private readonly cacheTtlMs: number) {}

  async warm(): Promise<number> {
    const rows = await searchBooks("", 100);
    return rows.length;
  }

  ttl(): number {
    return this.cacheTtlMs;
  }
}

export const restock = async (isbn: string, count: number): Promise<void> => {
  await prisma.book.update({ where: { isbn }, data: { stock: { increment: count } } });
};
