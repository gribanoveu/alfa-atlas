import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import type { BuiltRequest } from "./requestBuilder";

export type ExecutedResponse = {
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: string;
  durationMs: number;
};

/** Executes a request built by `buildRequest` via Tauri's native HTTP client
 * (not the webview's `fetch`) so calls to internal/corporate API hosts
 * aren't blocked by browser CORS — the same reason a tool like Postman needs
 * a native process rather than a plain webview request. */
export async function executeRequest(request: BuiltRequest): Promise<ExecutedResponse> {
  const start = performance.now();
  const response = await tauriFetch(request.url, {
    method: request.method,
    headers: request.headers,
    body: request.body ?? undefined,
  });
  const durationMs = performance.now() - start;

  const headers: Record<string, string> = {};
  response.headers.forEach((value, key) => {
    headers[key] = value;
  });

  const body = await response.text();

  return {
    status: response.status,
    statusText: response.statusText,
    headers,
    body,
    durationMs,
  };
}
