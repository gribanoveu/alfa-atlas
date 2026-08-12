import { invoke } from "@tauri-apps/api/core";
import { extensionOf } from "./fileExtensions";
import { isAsciiDocPath, monacoLanguageFor } from "./supportedFiles";

export type DictionaryDef = {
  id: string;
  title: string;
};

export type SpellcheckConfig = {
  enabled: boolean;
  dictionaries: Record<string, boolean>;
  skipCamelCase: boolean;
  /** When false, skip `.txt` files only. */
  checkTxt: boolean;
};

export type SpellIssue = {
  line: number;
  column: number;
  length: number;
  word: string;
};

export type DocKind = "markdown" | "asciidoc" | "plain";

export function isTxtPath(path: string): boolean {
  return extensionOf(path) === ".txt";
}

/** Which tokenizer the backend should use for a given file path. */
export function spellcheckKindFor(path: string): DocKind {
  if (isAsciiDocPath(path)) return "asciidoc";
  if (monacoLanguageFor(path) === "markdown") return "markdown";
  return "plain";
}

export function getDictionaries(): Promise<DictionaryDef[]> {
  return invoke<DictionaryDef[]>("get_dictionaries");
}

export function getSpellcheckConfig(): Promise<SpellcheckConfig> {
  return invoke<SpellcheckConfig>("get_spellcheck_config");
}

export function setSpellcheckConfig(
  config: SpellcheckConfig,
): Promise<void> {
  return invoke<void>("set_spellcheck_config", { config });
}

export function checkSpelling(
  text: string,
  docKind: DocKind,
  path: string,
): Promise<SpellIssue[]> {
  return invoke<SpellIssue[]>("check_spelling", { text, docKind, path });
}

export function suggestSpelling(word: string): Promise<string[]> {
  return invoke<string[]>("suggest_spelling", { word });
}

export function getCustomDictionaryWords(): Promise<string[]> {
  return invoke<string[]>("get_custom_dictionary_words");
}

export function addCustomDictionaryWord(word: string): Promise<void> {
  return invoke<void>("add_custom_dictionary_word", { word });
}

export function removeCustomDictionaryWord(word: string): Promise<void> {
  return invoke<void>("remove_custom_dictionary_word", { word });
}
