import { describe, expect, test } from "bun:test";
import {
  externalDocsOf,
  type JsonValue,
} from "../components/OpenApiExplorer/openApiModel";

const withUrl = (url: unknown): JsonValue =>
  ({ externalDocs: { url, description: "Docs" } }) as unknown as JsonValue;

describe("externalDocsOf", () => {
  test("accepts http and https links", () => {
    expect(externalDocsOf(withUrl("https://example.com/docs"))).toEqual({
      url: "https://example.com/docs",
      description: "Docs",
    });
    expect(externalDocsOf(withUrl("http://intranet.local/api"))?.url).toBe(
      "http://intranet.local/api",
    );
  });

  test("description is optional", () => {
    const node = { externalDocs: { url: "https://example.com" } } as unknown as JsonValue;
    expect(externalDocsOf(node)).toEqual({
      url: "https://example.com",
      description: null,
    });
  });

  // The spec being viewed may have been authored by someone else, and this
  // URL is handed to the OS "open" handler. Anything that is not a web link
  // must not produce a button at all.
  test.each([
    ["a local path", "/etc/passwd"],
    ["a relative path", "../../.atlas/llm_credentials.enc"],
    ["a home-relative path", "~/.atlas"],
    ["a file URL", "file:///etc/passwd"],
    ["a javascript URL", "javascript:alert(1)"],
    ["a data URL", "data:text/html,<script>alert(1)</script>"],
    ["an smb share", "smb://attacker.example/share"],
    ["a custom scheme", "myapp://do-something"],
    ["a windows path", "C:\\Windows\\System32"],
    ["a UNC path", "\\\\attacker.example\\share"],
    ["an empty string", ""],
    ["whitespace", "   "],
  ])("rejects %s", (_label, url) => {
    expect(externalDocsOf(withUrl(url))).toBeNull();
  });

  test("rejects a non-string url", () => {
    expect(externalDocsOf(withUrl(42))).toBeNull();
    expect(externalDocsOf(withUrl(null))).toBeNull();
    expect(externalDocsOf({ externalDocs: {} } as unknown as JsonValue)).toBeNull();
    expect(externalDocsOf({} as unknown as JsonValue)).toBeNull();
  });
});
