import express from "express";
import { readFile } from "fs/promises";
import * as path from "path";
import { validateUser, normalizeEmail } from "./validators";
import type { User } from "./models";

const MAX_RETRIES = 3;

export function createServer(port: number): void {
  const app = express();
  app.listen(port);
}

export async function loadConfig(configPath: string): Promise<string> {
  const resolved = path.resolve(configPath);
  const raw = await readFile(resolved, "utf-8");
  return raw;
}

export class UserService {
  private cache = new Map<string, User>();

  registerUser(email: string, name: string): User | null {
    const normalized = normalizeEmail(email);
    if (!validateUser(normalized, name)) {
      console.error("invalid user");
      return null;
    }
    const user = this.buildUser(normalized, name);
    this.cache.set(normalized, user);
    return user;
  }

  private buildUser(email: string, name: string): User {
    return { email, name } as User;
  }

  fetchAll = async (): Promise<User[]> => {
    return Array.from(this.cache.values());
  };
}

const retry = (fn: () => void) => {
  for (let i = 0; i < MAX_RETRIES; i++) {
    try {
      fn();
      return;
    } catch (e) {
      console.warn("retrying", e);
    }
  }
};

retry(() => createServer(8080));
