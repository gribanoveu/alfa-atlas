import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualHistory from "../lib/chatHistory";
import type { ChatSummary } from "../lib/chatHistory";
import type { ChatMessage } from "../lib/chatBlocks";

type Loaded = {
  messages: ChatMessage[];
  todos: unknown[];
  activePlanId: string | null;
  pendingResume: unknown | null;
};

let active: ChatSummary[] = [];
let archived: ChatSummary[] = [];
let contents: Record<string, Loaded> = {};
let listThrows: string | null = null;
let loadThrows: string | null = null;
let saved: Array<{ chatId: string; title: string; pendingResume: unknown }> = [];
let archiveCalls: Array<[string, boolean]> = [];
let extractCalls: string[] = [];
/** Resolvers for deferred `loadChatMessages`, to drive the stale-load race. */
let deferLoad = false;
let pendingLoads: Array<{ id: string; resolve: (l: Loaded) => void }> = [];

mock.module("../lib/chatHistory", () => ({
  ...actualHistory,
  listChats: async (_root: string, isArchived: boolean) => {
    if (listThrows) throw listThrows;
    return isArchived ? archived : active;
  },
  loadChatMessages: (id: string) => {
    if (loadThrows) return Promise.reject(loadThrows);
    if (deferLoad) {
      return new Promise<Loaded>((resolve) => pendingLoads.push({ id, resolve }));
    }
    return Promise.resolve(contents[id] ?? empty());
  },
  saveChat: async (
    _root: string,
    chatId: string,
    title: string,
    _m: unknown,
    _t: unknown,
    _p: unknown,
    pendingResume: unknown,
  ) => {
    saved.push({ chatId, title, pendingResume });
    return { id: chatId, title, updatedAt: 1 } as ChatSummary;
  },
  setChatArchived: async (id: string, on: boolean) => {
    archiveCalls.push([id, on]);
  },
  deriveChatTitle: () => "Заголовок",
}));
mock.module("../lib/memoryPipeline", () => ({
  memoryExtractTurn: async (_root: string, chatId: string) => {
    extractCalls.push(chatId);
  },
}));

const { useChatHistory } = await import("../hooks/useChatHistory");

function empty(): Loaded {
  return { messages: [], todos: [], activePlanId: null, pendingResume: null };
}
function chat(id: string): ChatSummary {
  return { id, title: id, updatedAt: 1 } as ChatSummary;
}
function msg(text: string): ChatMessage {
  return { role: "user", blocks: [{ kind: "text", text }] } as unknown as ChatMessage;
}

beforeEach(() => {
  active = [];
  archived = [];
  contents = {};
  listThrows = null;
  loadThrows = null;
  saved = [];
  archiveCalls = [];
  extractCalls = [];
  deferLoad = false;
  pendingLoads = [];
});

describe("useChatHistory — opening", () => {
  test("with no chats yet, a fresh id is minted immediately", async () => {
    // Consumers use `currentChatId` as a React remount key, so it must be
    // non-null right away rather than waiting for a first save.
    const { result } = renderHook(() => useChatHistory("/repo"));

    await waitFor(() => expect(result.current.currentChatId).not.toBeNull());
    expect(result.current.currentMessages).toEqual([]);
  });

  test("the most recent chat is opened on mount", async () => {
    active = [chat("recent"), chat("older")];
    contents.recent = { ...empty(), messages: [msg("привет")] };
    const { result } = renderHook(() => useChatHistory("/repo"));

    await waitFor(() => expect(result.current.currentChatId).toBe("recent"));
    expect(result.current.currentMessages).toHaveLength(1);
  });

  test("a paused turn is restored so it can be resumed", async () => {
    active = [chat("paused")];
    contents.paused = { ...empty(), pendingResume: { round: 2 } };
    const { result } = renderHook(() => useChatHistory("/repo"));

    await waitFor(() => expect(result.current.currentChatId).toBe("paused"));
    expect(result.current.currentPendingResume).toMatchObject({ round: 2 });
  });

  test("no repo means no chats and no id", async () => {
    const { result } = renderHook(() => useChatHistory(null));
    await waitFor(() => expect(result.current.activeChats).toEqual([]));
    expect(result.current.currentChatId).toBeNull();
  });

  test("a failing list degrades to an empty history rather than breaking the panel", async () => {
    listThrows = "chat store unreadable";
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).not.toBeNull());
    expect(result.current.activeChats).toEqual([]);
  });

  test("a chat whose messages fail to load opens empty", async () => {
    active = [chat("broken")];
    loadThrows = "row missing";
    const { result } = renderHook(() => useChatHistory("/repo"));

    await waitFor(() => expect(result.current.currentChatId).toBe("broken"));
    expect(result.current.currentMessages).toEqual([]);
  });
});

describe("useChatHistory — switching", () => {
  test("switching loads the other chat's messages", async () => {
    active = [chat("a"), chat("b")];
    contents.a = { ...empty(), messages: [msg("a")] };
    contents.b = { ...empty(), messages: [msg("b1"), msg("b2")] };
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    act(() => result.current.switchChat("b"));
    await waitFor(() => expect(result.current.currentMessages).toHaveLength(2));
    expect(result.current.currentChatId).toBe("b");
  });

  test("a load that lost the race does not overwrite the newer chat", async () => {
    // Clicking through the list quickly must not land an older chat's
    // messages under a newer chat's id.
    active = [chat("a"), chat("b"), chat("c")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    deferLoad = true;
    act(() => result.current.switchChat("b"));
    act(() => result.current.switchChat("c"));
    expect(pendingLoads).toHaveLength(2);

    await act(async () => {
      pendingLoads[0]?.resolve({ ...empty(), messages: [msg("из b")] });
    });

    expect(result.current.currentChatId).toBe("c");
    expect(result.current.currentMessages).toBeNull();
  });

  test("starting a new chat clears the transcript and mints a new id", async () => {
    active = [chat("a")];
    contents.a = { ...empty(), messages: [msg("привет")] };
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentMessages).toHaveLength(1));

    const previous = result.current.currentChatId;
    act(() => result.current.newChat());

    expect(result.current.currentChatId).not.toBe(previous);
    expect(result.current.currentMessages).toEqual([]);
    expect(result.current.currentPendingResume).toBeNull();
  });
});

describe("useChatHistory — archiving", () => {
  test("archiving the open chat moves to the next one", async () => {
    active = [chat("a"), chat("b")];
    contents.a = empty();
    contents.b = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    active = [chat("b")];
    await act(async () => {
      await result.current.archiveChat("a");
    });

    expect(archiveCalls).toEqual([["a", true]]);
    await waitFor(() => expect(result.current.currentChatId).toBe("b"));
  });

  test("archiving the last chat starts a fresh one", async () => {
    active = [chat("a")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    active = [];
    await act(async () => {
      await result.current.archiveChat("a");
    });

    expect(result.current.currentChatId).not.toBe("a");
    expect(result.current.currentMessages).toEqual([]);
  });

  test("archiving a chat that is not open leaves the open one alone", async () => {
    active = [chat("a"), chat("b")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    active = [chat("a")];
    await act(async () => {
      await result.current.archiveChat("b");
    });

    expect(result.current.currentChatId).toBe("a");
  });

  test("the archived list is only fetched when asked for", async () => {
    active = [chat("a")];
    contents.a = empty();
    archived = [chat("old")];
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));
    expect(result.current.archivedChats).toBeNull();

    await act(async () => {
      await result.current.loadArchived();
    });
    expect(result.current.archivedChats).toHaveLength(1);
    expect(result.current.archivedLoading).toBe(false);
  });
});

describe("useChatHistory — saving", () => {
  test("a settled turn is saved and starts memory extraction", async () => {
    active = [chat("a")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    const turn = [msg("вопрос"), msg("ответ")];
    await act(async () => {
      result.current.saveTurn(turn, []);
      await Promise.resolve();
    });

    expect(saved).toHaveLength(1);
    expect(saved[0]?.pendingResume).toBeNull();
    expect(result.current.currentMessages).toEqual(turn);
    await waitFor(() => expect(extractCalls).toEqual(["a"]));
  });

  test("a settled turn on a fresh chat updates currentMessages for export", async () => {
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).not.toBeNull());
    expect(result.current.currentMessages).toEqual([]);

    const turn = [msg("первый вопрос"), msg("первый ответ")];
    await act(async () => {
      result.current.saveTurn(turn, []);
      await Promise.resolve();
    });

    expect(result.current.currentMessages).toEqual(turn);
  });

  test("an empty transcript is not saved", async () => {
    active = [chat("a")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    act(() => result.current.saveTurn([], []));
    expect(saved).toEqual([]);
  });

  test("a paused turn saves what to resume from, and skips extraction", async () => {
    // Memory extraction only makes sense for a turn that concluded.
    active = [chat("a")];
    contents.a = empty();
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentChatId).toBe("a"));

    await act(async () => {
      result.current.savePendingApproval([msg("вопрос")], [], null, { round: 1 } as never);
      await Promise.resolve();
    });

    expect(saved[0]?.pendingResume).toMatchObject({ round: 1 });
    expect(result.current.currentPendingResume).toMatchObject({ round: 1 });
    expect(result.current.currentMessages).toEqual([msg("вопрос")]);
    expect(extractCalls).toEqual([]);
  });

  test("saving a settled turn clears an earlier pause in the same turn", async () => {
    active = [chat("a")];
    contents.a = { ...empty(), pendingResume: { round: 1 } };
    const { result } = renderHook(() => useChatHistory("/repo"));
    await waitFor(() => expect(result.current.currentPendingResume).not.toBeNull());

    await act(async () => {
      result.current.saveTurn([msg("вопрос")], []);
      await Promise.resolve();
    });

    expect(result.current.currentPendingResume).toBeNull();
  });
});
