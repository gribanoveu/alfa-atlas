import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "bun:test";
import { AssistantCompactionNotice } from "../components/RightDock/AssistantCompactionNotice";
import { COMPACTION_RUNNING_NOTICE_TEXT, formatCompactionNoticeText } from "../lib/contextCompaction";
import type { ChatMessage } from "../lib/chatBlocks";

afterEach(cleanup);

function notice(text: string, running: boolean): ChatMessage {
  return {
    id: "n1",
    role: "assistant",
    blocks: [{ type: "text", id: "b1", content: text }],
    streaming: false,
    isCompactionNotice: true,
    ...(running ? { compactionRunning: true } : {}),
  };
}

describe("AssistantCompactionNotice", () => {
  test("renders a card with a live region while the pass is running", () => {
    const { container } = render(<AssistantCompactionNotice message={notice(COMPACTION_RUNNING_NOTICE_TEXT, true)} />);
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.getByText(COMPACTION_RUNNING_NOTICE_TEXT)).toBeTruthy();
    expect(container.querySelector(".assistant-compaction-card")).toBeTruthy();
    expect(container.querySelector(".assistant-loading-bars")).toBeTruthy();
  });

  test("collapses to the pill once the pass has settled", () => {
    const text = formatCompactionNoticeText(1, 4);
    const { container } = render(<AssistantCompactionNotice message={notice(text, false)} />);
    expect(screen.getByText(text)).toBeTruthy();
    expect(container.querySelector(".assistant-chat-compaction-notice")).toBeTruthy();
    // No spinner and no live region: nothing is happening any more.
    expect(container.querySelector(".assistant-compaction-card")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
