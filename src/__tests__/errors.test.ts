import { describe, expect, test } from "bun:test";
import { toMessage } from "../lib/errors";

describe("toMessage", () => {
  test("an Error yields its message, not its stack or name", () => {
    expect(toMessage(new Error("boom"))).toBe("boom");
  });

  test("a rejected invoke() yields the string verbatim", () => {
    // Tauri commands here return `Result<_, String>`, so a rejection is the
    // bare string — the common case, and the reason this isn't just
    // `String(e.message)`.
    expect(toMessage("no project is open")).toBe("no project is open");
  });

  test("anything else is stringified rather than lost", () => {
    expect(toMessage(null)).toBe("null");
    expect(toMessage(undefined)).toBe("undefined");
    expect(toMessage(42)).toBe("42");
    expect(toMessage({ code: 1 })).toBe("[object Object]");
  });

  test("a subclass of Error still yields its message", () => {
    class Custom extends Error {}
    expect(toMessage(new Custom("custom"))).toBe("custom");
  });
});
