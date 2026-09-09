import { describe, expect, mock, test } from "bun:test";
import { render } from "@testing-library/react";
import type { ChatMessage } from "../lib/chatBlocks";

mock.module("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));

// Same stub, same reason as `assistantMarkdownRender.test.tsx`: importing the
// message component reaches `AssistantVisualCard`, and the real module drags
// in the PlantUML engine, which does not survive happy-dom at import time.
mock.module("../lib/diagramRender", () => ({
  renderDiagram: async () => ({ kind: "ok", svg: "<svg />" }),
}));

// Counts how often a settled bubble's Markdown is actually re-parsed. Stubbed
// rather than measured through the real Streamdown because the point of the
// test is the render count, not the output.
let markdownRenders: string[] = [];
mock.module("../components/RightDock/AssistantMarkdown", () => ({
  AssistantMarkdown: ({ content }: { content: string }) => {
    markdownRenders.push(content);
    return <div className="stub-md">{content}</div>;
  },
}));

const { AssistantMessage } = await import(
  "../components/RightDock/AssistantConversation/AssistantMessage"
);

function assistantMessage(id: string, text: string): ChatMessage {
  return {
    id,
    role: "assistant",
    streaming: false,
    blocks: [{ type: "text", id: `${id}-b`, content: text }],
  };
}

const noop = () => {};
const handlers = {
  chatId: "chat-1",
  docsRoot: "/repo/docs",
  repoRoot: "/repo",
  sending: false,
  retryState: null,
  onAnswerArtifact: noop,
  onAnswerAskUser: noop,
  onDecideToolCall: noop,
  onRetryWithCompaction: noop,
  onStartPlan: noop,
  onOpenPlan: noop,
  onOpenArtifact: noop,
  onOpenVisual: noop,
  onVisualRenderError: noop,
  onRedrawVisual: noop,
};

/** The backend emits one event per streamed token, so this is the difference
 * between a chat that stays responsive on a long conversation and one that
 * re-parses its entire history per character. `updateLastAssistantBlocks`
 * hands settled messages through by reference, so `memo` can skip them — but
 * only while every callback prop stays referentially stable. This test fails
 * the moment someone reintroduces an inline arrow at the call site. */
describe("AssistantMessage — memoization", () => {
  test("a settled message is not re-rendered when its props are unchanged", () => {
    markdownRenders = [];
    const settled = assistantMessage("m1", "готовый ответ");

    const { rerender } = render(<AssistantMessage message={settled} {...handlers} />);
    expect(markdownRenders).toEqual(["готовый ответ"]);

    // Same message reference, same handlers — exactly what a delta landing on
    // a *later* bubble hands this one.
    rerender(<AssistantMessage message={settled} {...handlers} />);
    expect(markdownRenders).toEqual(["готовый ответ"]);
  });

  test("a changed message still re-renders", () => {
    markdownRenders = [];
    const first = assistantMessage("m1", "часть");

    const { rerender } = render(<AssistantMessage message={first} {...handlers} />);
    rerender(<AssistantMessage message={assistantMessage("m1", "часть вторая")} {...handlers} />);

    expect(markdownRenders).toEqual(["часть", "часть вторая"]);
  });

  test("an unstable callback prop defeats the memo — the failure this guards", () => {
    markdownRenders = [];
    const settled = assistantMessage("m1", "готовый ответ");

    const { rerender } = render(
      <AssistantMessage message={settled} {...handlers} onOpenPlan={() => {}} />,
    );
    rerender(<AssistantMessage message={settled} {...handlers} onOpenPlan={() => {}} />);

    expect(markdownRenders).toHaveLength(2);
  });
});
