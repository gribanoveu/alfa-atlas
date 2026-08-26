import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { DictionaryDef, SpellcheckConfig } from "../lib/spellcheck";
import * as actualSpellcheck from "../lib/spellcheck";

const DICTS = [
  { id: "ru", label: "Русский" },
  { id: "en", label: "English" },
] as unknown as DictionaryDef[];

let stored: SpellcheckConfig;
let words: string[] = [];
let saveFails: string | null = null;
let saves: SpellcheckConfig[] = [];
let addFails: string | null = null;

mock.module("../lib/spellcheck", () => ({
  ...actualSpellcheck,
  getDictionaries: async () => DICTS,
  getSpellcheckConfig: async () => stored,
  setSpellcheckConfig: async (c: SpellcheckConfig) => {
    if (saveFails) throw saveFails;
    saves.push(c);
    stored = c;
  },
  getCustomDictionaryWords: async () => words,
  addCustomDictionaryWord: async (w: string) => {
    if (addFails) throw addFails;
    words = [...words, w];
  },
  removeCustomDictionaryWord: async (w: string) => {
    words = words.filter((x) => x !== w);
  },
}));

const { useSpellcheckSettings } = await import("../hooks/useSpellcheckSettings");

beforeEach(() => {
  stored = { enabled: true, dictionaries: {}, skipCamelCase: true, checkTxt: false } as SpellcheckConfig;
  words = [];
  saveFails = null;
  saves = [];
  addFails = null;
});

describe("useSpellcheckSettings", () => {
  test("loads dictionaries, config and the custom word list", async () => {
    words = ["Атлас"];
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    expect(result.current.dictionaries).toHaveLength(2);
    await waitFor(() => expect(result.current.words).toEqual(["Атлас"]));
  });

  test("a dictionary with no explicit setting counts as enabled", async () => {
    // A newly shipped dictionary is on until turned off.
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    expect(result.current.isDictionaryEnabled(DICTS[0]!)).toBe(true);
  });

  test("an explicit false wins", async () => {
    stored = { ...stored, dictionaries: { ru: false } } as SpellcheckConfig;
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    expect(result.current.isDictionaryEnabled(DICTS[0]!)).toBe(false);
  });

  test("toggling a dictionary leaves the others untouched", async () => {
    stored = { ...stored, dictionaries: { en: false } } as SpellcheckConfig;
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    await act(async () => {
      result.current.toggleDictionary(DICTS[0]!, false);
    });

    expect(saves.at(-1)?.dictionaries).toEqual({ en: false, ru: false });
  });

  test("the config change is announced to the app after it lands", async () => {
    const seen: SpellcheckConfig[] = [];
    const { result } = renderHook(() => useSpellcheckSettings((c) => seen.push(c)));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    await act(async () => {
      result.current.toggleEnabled(false);
    });

    expect(seen.at(-1)?.enabled).toBe(false);
  });

  test("a failed write rolls the toggle back and is not announced", async () => {
    const seen: SpellcheckConfig[] = [];
    const { result } = renderHook(() => useSpellcheckSettings((c) => seen.push(c)));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    saveFails = "settings file is read-only";

    await act(async () => {
      result.current.toggleEnabled(false);
    });

    expect(result.current.config?.enabled).toBe(true);
    expect(result.current.error).toBe("settings file is read-only");
    expect(seen).toHaveLength(0);
  });

  test("adding a word clears the input and refreshes the list", async () => {
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    act(() => result.current.setNewWord("  Атлас  "));
    await act(async () => {
      await result.current.addWord();
    });

    // Trimmed on the way in.
    expect(words).toEqual(["Атлас"]);
    expect(result.current.newWord).toBe("");
    expect(result.current.words).toEqual(["Атлас"]);
  });

  test("a blank word is not submitted", async () => {
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    act(() => result.current.setNewWord("   "));
    await act(async () => {
      await result.current.addWord();
    });

    expect(words).toEqual([]);
  });

  test("a failed add keeps the typed word so it is not lost", async () => {
    addFails = "dictionary is locked";
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    act(() => result.current.setNewWord("Атлас"));
    await act(async () => {
      await result.current.addWord();
    });

    expect(result.current.error).toBe("dictionary is locked");
    expect(result.current.newWord).toBe("Атлас");
  });

  test("removing a word drops it from the list", async () => {
    words = ["Атлас", "Тауri"];
    const { result } = renderHook(() => useSpellcheckSettings());
    await waitFor(() => expect(result.current.words).toHaveLength(2));

    await act(async () => {
      await result.current.removeWord("Атлас");
    });

    expect(result.current.words).toEqual(["Тауri"]);
  });
});
