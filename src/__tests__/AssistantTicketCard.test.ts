import { describe, expect, test } from "bun:test";
import type { ToolCallBlock } from "../lib/chatBlocks";
import { isTicketToolBlock } from "../components/RightDock/AssistantTicketCard";

function block(name: string, argumentsJson: string): ToolCallBlock {
  return { type: "toolCall", id: "1", name, argumentsJson, status: "done" };
}

describe("isTicketToolBlock", () => {
  test("claims the write ops", () => {
    expect(isTicketToolBlock(block("artifact", '{"op":"create","title":"X"}'))).toBe(true);
    expect(isTicketToolBlock(block("artifact", '{"op":"update","id":"a"}'))).toBe(true);
  });

  // A card per read would bury the conversation, and there is nothing new
  // for the user to open.
  test("leaves the read ops to the ordinary tool line", () => {
    expect(isTicketToolBlock(block("artifact", '{"op":"read","id":"a"}'))).toBe(false);
    expect(isTicketToolBlock(block("artifact", '{"op":"list"}'))).toBe(false);
  });

  test("ignores other tools", () => {
    expect(isTicketToolBlock(block("requestArtifact", '{"kind":"httpRequest"}'))).toBe(false);
    expect(isTicketToolBlock(block("visualize", '{"op":"create"}'))).toBe(false);
  });

  // Arguments arrive as a model-produced string; a truncated stream must
  // not throw its way out of the render.
  test("survives arguments that are not valid JSON", () => {
    expect(isTicketToolBlock(block("artifact", '{"op":"crea'))).toBe(false);
    expect(isTicketToolBlock(block("artifact", ""))).toBe(false);
  });
});
