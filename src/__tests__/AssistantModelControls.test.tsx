import {
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, test } from "bun:test";
import { AssistantModelControls } from "../components/RightDock/AssistantConversation/AssistantModelControls";
import type { ResolvedLlmProvider } from "../lib/llm";

afterEach(cleanup);

function provider(
  overrides: Partial<ResolvedLlmProvider> = {},
): ResolvedLlmProvider {
  return {
    id: "provider",
    label: "Provider",
    baseUrl: "https://example.test",
    isSystem: false,
    model: null,
    knownModels: ["fast-model", "deep-model"],
    trustedCertPem: null,
    trustedCertOverride: null,
    hasBundledCert: false,
    limit: { context: 100_000, output: 10_000 },
    requestHeaders: {},
    temperature: null,
    maxTokens: null,
    reasoningEffort: null,
    ...overrides,
  };
}

describe("AssistantModelControls", () => {
  test("refreshes the catalog when opening and persists a model choice", () => {
    let refreshes = 0;
    const updates: unknown[] = [];
    render(
      <AssistantModelControls
        providerId="provider"
        activeProvider={provider()}
        sending={false}
        accessMode="docsOnly"
        accessModeBusy={false}
        conversationMode="agent"
        contextTokens={12_500}
        lastRequestTokens={null}
        onConversationModeChange={() => {}}
        onAccessModeChange={() => {}}
        updateProviderConfig={async (id, patch) => {
          updates.push([id, patch]);
        }}
        refreshLlmSetup={async () => {
          refreshes += 1;
        }}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "Авто (первая доступная)",
      }),
    );
    expect(refreshes).toBe(1);
    fireEvent.click(screen.getByRole("option", { name: "deep-model" }));
    expect(updates).toEqual([
      ["provider", { model: "deep-model" }],
    ]);
  });

  test("keeps access and mode controls disabled while sending", () => {
    render(
      <AssistantModelControls
        providerId="provider"
        activeProvider={provider({ model: "fast-model" })}
        sending
        accessMode="docsOnly"
        accessModeBusy={false}
        conversationMode="plan"
        contextTokens={90_000}
        lastRequestTokens={80_000}
        onConversationModeChange={() => {}}
        onAccessModeChange={() => {}}
        updateProviderConfig={async () => {}}
        refreshLlmSetup={async () => {}}
      />,
    );

    expect(
      screen.getByRole("button", { name: "План" }).hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen
        .getByRole("radio", { name: "Документация" })
        .hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen
        .getByRole("button", { name: "fast-model" })
        .hasAttribute("disabled"),
    ).toBe(true);
  });
});
