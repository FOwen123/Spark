import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { appUrl } from "./util";

export function token() {
  return new URL(appUrl()).hash.replace("#token=", "");
}

/** An MCP client connected to the test server, as an agent named `name`. */
export async function connectAgent(name = "e2e-agent") {
  const url = new URL("/mcp", appUrl());
  const transport = new StreamableHTTPClientTransport(url, { requestInit: { headers: { Authorization: `Bearer ${token()}` } } });
  const client = new Client({ name, version: "1.0.0" });
  await client.connect(transport);
  // End the MCP session on close, like a well-behaved agent, so presence clears.
  const close = client.close.bind(client);
  client.close = async () => {
    await transport.terminateSession().catch(() => {});
    await close();
  };
  return client;
}

type Content = { type: string; text?: string; data?: string; mimeType?: string };

export async function call(client: Client, name: string, args: Record<string, unknown> = {}) {
  const r = (await client.callTool({ name, arguments: args })) as { content: Content[]; isError?: boolean };
  if (r.isError) throw new Error(`${name}: ${r.content.map((c) => c.text).join(" ")}`);
  return r.content;
}

export async function callJson<T = Record<string, unknown>>(client: Client, name: string, args: Record<string, unknown> = {}): Promise<T> {
  const content = await call(client, name, args);
  return JSON.parse(content[content.length - 1].text ?? "null") as T;
}
