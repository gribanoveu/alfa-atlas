import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ContextBreakdown } from "../hooks/useLlmChat";
import type { ResolvedLlmProvider } from "../lib/llm";
import { AssistantModelControls } from "../components/RightDock/AssistantConversation/AssistantModelControls";

afterEach(cleanup);

const breakdown: ContextBreakdown = {
  systemPrompt: 3_000,
  toolSchemas: 9_000,
  chat: 20_000,
  skills: 12_000,
  total: 44_000,
};

const provider = {
  id: "openai",
  model: "gpt-x",
  knownModels: [],
  limit: { context: 100_000 },
} as unknown as ResolvedLlmProvider;

function renderBar(over: Partial<{ contextTokens: number; lastRequestTokens: number | null }> = {}) {
  return render(
    <AssistantModelControls
      providerId="openai"
      activeProvider={provider}
      sending={false}
      accessMode={"docsOnly" as never}
      accessModeBusy={false}
      conversationMode={"agent" as never}
      contextTokens={over.contextTokens ?? breakdown.total}
      contextBreakdown={breakdown}
      lastRequestTokens={over.lastRequestTokens ?? null}
      onConversationModeChange={() => {}}
      onAccessModeChange={() => {}}
      updateProviderConfig={async () => {}}
      refreshLlmSetup={async () => {}}
    />,
  );
}

describe("context ring breakdown popover", () => {
  test("stays closed until the ring is clicked, then lists where the tokens go", () => {
    renderBar();
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /100K/ }));
    const dialog = screen.getByRole("dialog");
    // Largest first, so the row worth acting on is the one read first.
    const labels = [...dialog.querySelectorAll(".assistant-context-popover-label")].map(
      (el) => el.textContent,
    );
    expect(labels).toEqual([
      "История чата",
      "Скиллы",
      "Схемы инструментов",
      "Системный промпт",
      "Свободно",
    ]);
    expect(dialog.textContent).toContain("56K");
    // The small per-turn blocks are named rather than folded into a bucket
    // that would look precise without being it.
    expect(dialog.textContent).toContain("в разбивку не входят");
  });

  // While a turn is in flight the ring is floored by the provider's own
  // count, which sees things no client-side projection does — the rows must
  // not silently disagree with the label above them.
  test("shows the gap to the provider's own count as its own row", () => {
    renderBar({ contextTokens: 50_000 });
    fireEvent.click(screen.getByRole("button", { name: /100K/ }));
    expect(screen.getByRole("dialog").textContent).toContain("Прочее (замер провайдера)");
  });

  test("Escape closes it", () => {
    renderBar();
    fireEvent.click(screen.getByRole("button", { name: /100K/ }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
