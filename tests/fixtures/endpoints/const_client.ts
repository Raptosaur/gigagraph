const BREWS_API = "/brews";

export async function loadViaConst(): Promise<unknown> {
  const res = await fetch(BREWS_API);
  return res.json();
}
