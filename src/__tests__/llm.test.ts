import { describe, expect, test } from "bun:test";
import { mergeKnownModels, resolveOpenAiCompatibleEndpoints } from "../lib/llm";

describe("resolveOpenAiCompatibleEndpoints", () => {
  test("appends standard OpenAI-compatible paths", () => {
    expect(resolveOpenAiCompatibleEndpoints("https://openrouter.ai/api/v1")).toEqual({
      chat: "https://openrouter.ai/api/v1/chat/completions",
      models: "https://openrouter.ai/api/v1/models",
    });
  });

  test("strips trailing slashes before appending", () => {
    expect(resolveOpenAiCompatibleEndpoints("https://api.openai.com/v1/")).toEqual({
      chat: "https://api.openai.com/v1/chat/completions",
      models: "https://api.openai.com/v1/models",
    });
  });

  test("returns null for blank input", () => {
    expect(resolveOpenAiCompatibleEndpoints("")).toBeNull();
    expect(resolveOpenAiCompatibleEndpoints(null)).toBeNull();
  });
});

describe("mergeKnownModels", () => {
  test("dedupes and trims", () => {
    expect(mergeKnownModels(["a"], [" b ", "a", ""])).toEqual(["a", "b"]);
  });
});
