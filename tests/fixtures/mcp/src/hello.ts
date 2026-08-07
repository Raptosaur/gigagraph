export function hello(name: string): string {
  return "hi " + name;
}

export function shoutHello(name: string): string {
  return hello(name).toUpperCase();
}
