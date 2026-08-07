export async function fetchItems(): Promise<unknown> {
  const res = await fetch("http://pysvc.internal:5000/items");
  return res.json();
}

export async function syncOrders(): Promise<string> {
  const res = await fetch("http://gosvc.internal:8080/orders");
  return res.text();
}
