import axios from "axios";

export async function loadUser(id: string): Promise<unknown> {
  const res = await fetch(`/users/${id}`);
  return res.json();
}

export async function createUser(): Promise<void> {
  await axios.post("/users", {});
}

export async function pollExternal(): Promise<void> {
  await fetch("https://api.example.com/metrics");
}
