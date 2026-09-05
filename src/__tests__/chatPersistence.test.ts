import { describe, expect, test } from "bun:test";
import fixture from "../lib/chatPersistence.contract.json";
import { chatMessagesToJson, chatMessagesToMarkdown } from "../lib/chatExport";
import {
  CHAT_MESSAGE_SCHEMA_VERSION,
  ChatPersistenceContractError,
  decodePersistedChatMessages,
  encodePersistedChatMessages,
} from "../lib/chatPersistence";

describe("persisted chat message contract", () => {
  test("decodes the shared fixture and preserves export-visible frontend shapes", () => {
    const messages = decodePersistedChatMessages(fixture.versionedMessages);

    expect(messages[0]).toMatchObject({
      role: "user",
      content: "Remember that release branches require two approvals.",
      isPlanExecutionStart: true,
    });
    expect(messages[1]).toMatchObject({
      role: "assistant",
      blocks: [
        { type: "reasoning" },
        { type: "text", content: "Release branches require two approvals." },
        { type: "toolCall", status: "done" },
        { type: "steer", text: "Keep the answer concise." },
        { type: "text", content: "I will keep future release guidance concise." },
      ],
      usage: { totalTokens: 59 },
    });

    const markdown = chatMessagesToMarkdown("Contract", messages, []);
    expect(markdown).toContain("Release branches require two approvals.");
    expect(markdown).toContain("Уточнение пользователя: Keep the answer concise.");
    const exported = JSON.parse(chatMessagesToJson("chat-1", "Contract", messages, []));
    expect(exported.messages).toEqual(messages);
  });

  test("migrates legacy unversioned and flat-assistant messages centrally", () => {
    const messages = decodePersistedChatMessages(fixture.legacyUnversionedMessages);

    expect(messages[0]).toMatchObject({ id: "legacy-user", role: "user" });
    expect(messages[1]).toEqual({
      id: "legacy-assistant",
      role: "assistant",
      blocks: [
        {
          type: "text",
          id: "legacy-assistant:legacy-text",
          content: "Legacy assistant text remains readable.",
        },
      ],
    });
    expect(encodePersistedChatMessages(messages)).toEqual(
      messages.map((message) => ({ schemaVersion: CHAT_MESSAGE_SCHEMA_VERSION, message })),
    );
  });

  test("rejects unknown versions instead of silently misreading them", () => {
    expect(() =>
      decodePersistedChatMessages([
        { schemaVersion: 99, message: { id: "m", role: "user", content: "future" } },
      ]),
    ).toThrow(ChatPersistenceContractError);
  });
});
