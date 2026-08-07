import { JSONRPCServer } from "json-rpc-2.0";

const server = new JSONRPCServer();

function echoMessage(params: { msg: string }) {
  return params.msg;
}

server.addMethod("echoMessage", echoMessage);
server.addMethod("sumNumbers", (p: { a: number; b: number }) => p.a + p.b);
