// src/index.ts
import { get as config } from "flowcatalyst:function/config";
import { context } from "flowcatalyst:function/invocation";
import { info } from "flowcatalyst:function/log";
async function handle(request) {
  const url = new URL(request.url);
  if (request.method === "POST" && url.pathname === "/events/greeting-requested") {
    return greetingRequested(request);
  }
  const name = pathParam("name");
  if (request.method === "GET" && name !== void 0) {
    return hello(name);
  }
  return Response.json({ error: "not found" }, { status: 404 });
}
function hello(name) {
  const greeting = config("GREETING") ?? "Hello";
  return Response.json({ message: `${greeting}, ${name}!` });
}
async function greetingRequested(request) {
  const event = await request.json();
  info(`event ${event.id} (${event.type})`);
  return new Response(null);
}
function pathParam(name) {
  return context().pathParams.find(([key]) => key === name)?.[1];
}
export {
  handle as default
};
