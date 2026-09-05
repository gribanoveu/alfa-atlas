import type { ChatMessage, MessageBlock } from "./chatBlocks";

export const CHAT_MESSAGE_SCHEMA_VERSION = 1 as const;

/** Stable wire/storage envelope for one frontend chat message. */
export type PersistedChatMessageV1 = {
  schemaVersion: typeof CHAT_MESSAGE_SCHEMA_VERSION;
  message: ChatMessage;
};

export type PersistedChatMessage = PersistedChatMessageV1;

export class ChatPersistenceContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ChatPersistenceContractError";
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(record: Record<string, unknown>, key: string, path: string): string {
  const value = record[key];
  if (typeof value !== "string") {
    throw new ChatPersistenceContractError(`${path}.${key} must be a string`);
  }
  return value;
}

function normalizeBlock(value: unknown, path: string): MessageBlock {
  if (!isRecord(value)) {
    throw new ChatPersistenceContractError(`${path} must be an object`);
  }
  const type = requireString(value, "type", path);
  requireString(value, "id", path);
  switch (type) {
    case "text":
    case "reasoning":
      requireString(value, "content", path);
      return value as MessageBlock;
    case "steer":
      requireString(value, "text", path);
      return value as MessageBlock;
    case "toolCall": {
      requireString(value, "name", path);
      requireString(value, "argumentsJson", path);
      const status = requireString(value, "status", path);
      if (!["pendingApproval", "running", "done", "error"].includes(status)) {
        throw new ChatPersistenceContractError(`${path}.status is not a supported tool-call status`);
      }
      return value as MessageBlock;
    }
    default:
      throw new ChatPersistenceContractError(`${path}.type is not a supported message block`);
  }
}

/** Runtime-checks the contract Rust readers depend on, preserving optional
 * and future additive fields verbatim. The flat assistant `content` branch
 * migrates the pre-block transcript shape into today's `blocks` union. */
function normalizeMessage(value: unknown, path: string): ChatMessage {
  if (!isRecord(value)) {
    throw new ChatPersistenceContractError(`${path} must be an object`);
  }
  const id = requireString(value, "id", path);
  const role = requireString(value, "role", path);
  if (role === "user") {
    requireString(value, "content", path);
    return value as ChatMessage;
  }
  if (role !== "assistant") {
    throw new ChatPersistenceContractError(`${path}.role must be user or assistant`);
  }
  if (Array.isArray(value.blocks)) {
    value.blocks.forEach((block, index) => normalizeBlock(block, `${path}.blocks[${index}]`));
    return value as ChatMessage;
  }
  if (typeof value.content === "string") {
    const { content, ...rest } = value;
    return {
      ...rest,
      id,
      role: "assistant",
      blocks: [{ type: "text", id: `${id}:legacy-text`, content }],
    } as ChatMessage;
  }
  throw new ChatPersistenceContractError(`${path}.blocks must be an array`);
}

export function encodePersistedChatMessages(messages: ChatMessage[]): PersistedChatMessage[] {
  return messages.map((message) => ({
    schemaVersion: CHAT_MESSAGE_SCHEMA_VERSION,
    message,
  }));
}

/** Accepts current envelopes and legacy unversioned message objects. Every
 * result is a current `ChatMessage`, so migration stays at the load boundary
 * instead of leaking schema probes into renderers, export, or hooks. */
export function decodePersistedChatMessages(value: unknown): ChatMessage[] {
  if (!Array.isArray(value)) {
    throw new ChatPersistenceContractError("persisted chat messages must be an array");
  }
  return value.map((item, index) => {
    const path = `messages[${index}]`;
    if (!isRecord(item) || !Object.prototype.hasOwnProperty.call(item, "schemaVersion")) {
      return normalizeMessage(item, path);
    }
    if (item.schemaVersion !== CHAT_MESSAGE_SCHEMA_VERSION) {
      throw new ChatPersistenceContractError(
        `${path}.schemaVersion ${String(item.schemaVersion)} is not supported`,
      );
    }
    return normalizeMessage(item.message, `${path}.message`);
  });
}
