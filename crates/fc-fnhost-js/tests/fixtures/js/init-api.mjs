import { get } from "flowcatalyst:function/config";
const greeting = get("GREETING");
export default () => new Response(greeting);
