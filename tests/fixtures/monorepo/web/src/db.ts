import { Pool } from "pg";

const pool = new Pool();

export async function queryRows(sql: string, params: unknown[]): Promise<unknown[]> {
  const result = await pool.query(sql, params);
  return result.rows;
}

export function sumEvens(nums: number[]): number {
  let total = 0;
  for (const n of nums) {
    if (n % 2 === 0) {
      total += n;
    }
  }
  return total;
}
