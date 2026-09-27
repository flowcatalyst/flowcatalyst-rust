// {{project-name}}: a FlowCatalyst function. See README.md.
import { get as config } from "flowcatalyst:function/config";
import { context } from "flowcatalyst:function/invocation";
import { info } from "flowcatalyst:function/log";

/**
 * @param {Request} request
 * @returns {Promise<Response>}
 */
export default async function handle(request) {
  const url = new URL(request.url);
  if (request.method === "POST" && url.pathname === "/events/greeting-requested") {
    return greetingRequested(request);
  }
  const name = pathParam("name");
  if (request.method === "GET" && name !== undefined) {
    return hello(name);
  }
  return Response.json({ error: "not found" }, { status: 404 });
}

/**
 * `GET /hello/{name}`: a greeting, from the `GREETING` config value.
 * @param {string} name
 */
function hello(name) {
  const greeting = config("GREETING") ?? "Hello";
  return Response.json({ message: `${greeting}, ${name}!` });
}

/**
 * `POST /events/greeting-requested`: a subscription delivery. An empty 200
 * acknowledges it; a throw fails the attempt (500), and `429` with
 * `Retry-After: <seconds>` asks for it again later.
 * @param {Request} request
 */
async function greetingRequested(request) {
  const event = await request.json();
  info(`event ${event.id} (${event.type})`);
  return new Response(null);
}

/**
 * A parameter the matched endpoint's path pattern bound.
 * @param {string} name
 */
function pathParam(name) {
  return context().pathParams.find(([key]) => key === name)?.[1];
}
