// {{project-name}}: a FlowCatalyst function. See README.md.
import { get as config } from "flowcatalyst:function/config";
import { context } from "flowcatalyst:function/invocation";
import { info } from "flowcatalyst:function/log";

/** A subscription delivery's body: the platform's event envelope. */
interface EventEnvelope {
  id: string;
  type: string;
  data?: unknown;
}

export default async function handle(request: Request): Promise<Response> {
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

/** `GET /hello/{name}`: a greeting, from the `GREETING` config value. */
function hello(name: string): Response {
  const greeting = config("GREETING") ?? "Hello";
  return Response.json({ message: `${greeting}, ${name}!` });
}

/**
 * `POST /events/greeting-requested`: a subscription delivery. An empty 200
 * acknowledges it; a throw fails the attempt (500), and `429` with
 * `Retry-After: <seconds>` asks for it again later.
 */
async function greetingRequested(request: Request): Promise<Response> {
  const event = (await request.json()) as EventEnvelope;
  info(`event ${event.id} (${event.type})`);
  return new Response(null);
}

/** A parameter the matched endpoint's path pattern bound. */
function pathParam(name: string): string | undefined {
  return context().pathParams.find(([key]) => key === name)?.[1];
}
