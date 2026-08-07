export function validateUser(email: string, name: string): boolean {
  return email.includes("@") && name.length > 0;
}

export function normalizeEmail(email: string): string {
  return email.trim().toLowerCase();
}
