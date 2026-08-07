import { JSONRPCClient } from "json-rpc-2.0";

const client = new JSONRPCClient(async () => {});

export function callEcho(): Promise<string> {
  return client.request("echoMessage", { msg: "hi" });
}
