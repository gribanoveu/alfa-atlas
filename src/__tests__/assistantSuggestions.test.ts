import { describe, expect, test } from "bun:test";
import {
  ASSISTANT_SUGGESTIONS,
  buildSuggestionContext,
  needsAccessUpgrade,
  needsSuggestionForm,
  prefillValues,
  renderSuggestionText,
  suggestionAccess,
  suggestionBlockedReason,
  suggestionFormComplete,
  suggestionsForMode,
  visibleSuggestions,
  type AssistantSuggestion,
  type SuggestionContext,
} from "../lib/assistantSuggestions";

function flatten(suggestions: AssistantSuggestion[]): AssistantSuggestion[] {
  return suggestions.flatMap((s) => [s, ...flatten(s.followUps ?? [])]);
}

const ALL = flatten(ASSISTANT_SUGGESTIONS);

function placeholdersIn(text: string): string[] {
  return [...text.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1] as string);
}

const NO_CONTEXT: SuggestionContext = {
  conversationMode: "agent",
  activeFilePath: null,
  isMethodDoc: false,
  hasUncommittedChanges: false,
};

const EVERYTHING: SuggestionContext = {
  conversationMode: "agent",
  activeFilePath: "operations/createSignOperation/createSignOperation.adoc",
  isMethodDoc: true,
  hasUncommittedChanges: true,
};

describe("ASSISTANT_SUGGESTIONS structure", () => {
  test("ids are unique across the whole tree", () => {
    const ids = ALL.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  test("every {{placeholder}} is a declared input, and every input is used", () => {
    for (const s of ALL) {
      const declared = (s.inputs ?? []).map((i) => i.key).sort();
      const used = [...new Set(placeholdersIn(s.text))].sort();
      expect({ id: s.id, keys: used }).toEqual({ id: s.id, keys: declared });
    }
  });

  test("no suggestion leaves the prompt dangling for the user to finish", () => {
    // The whole point of `inputs`: a chip must never end mid-sentence
    // ("Название метода - ") hoping the user notices they should continue.
    for (const s of ALL) {
      expect({ id: s.id, tail: s.text.trimEnd().slice(-2) }).not.toEqual({
        id: s.id,
        tail: "- ",
      });
      expect(s.text.trimEnd().endsWith("-")).toBe(false);
    }
  });

  test("read-only suggestions never ask for a write mode", () => {
    for (const s of ALL) {
      if (!s.writes) expect(s.mode).not.toBe("agent");
    }
  });

  test("every suggestion declares the mode it belongs to", () => {
    for (const s of ALL) {
      expect({ id: s.id, mode: s.mode }).toEqual({
        id: s.id,
        mode: expect.stringMatching(/^(agent|plan|question)$/) as unknown as typeof s.mode,
      });
    }
  });

  test("access defaults to docs-only", () => {
    const fullRepo = ALL.filter((s) => suggestionAccess(s) === "fullRepo").map((s) => s.id);
    expect(fullRepo.sort()).toEqual([
      "describe-algorithm",
      "describe-errors",
      "explain-feature",
      "plan-feature-docs",
      "sync-with-code",
    ]);
  });
});

describe("renderSuggestionText", () => {
  const suggestion = ALL.find((s) => s.id === "new-method-doc")!;

  test("substitutes every placeholder and leaves no braces", () => {
    const text = renderSuggestionText(suggestion, { method: "createSignOperationV2" });
    expect(text).toContain("createSignOperationV2.adoc");
    expect(text).not.toContain("{{");
  });

  test("trims the user's value", () => {
    const text = renderSuggestionText(suggestion, { method: "  createSignOperationV2\n" });
    expect(text).toContain("метод createSignOperationV2:");
  });

  test("an omitted optional value collapses instead of leaking braces", () => {
    const optional: AssistantSuggestion = {
      id: "t",
      label: "t",
      mode: "agent",
      writes: false,
      text: "начало {{tail}}",
      inputs: [{ key: "tail", label: "t", placeholder: "" }],
    };
    expect(renderSuggestionText(optional, {})).toBe("начало ");
  });
});

describe("visibleSuggestions", () => {
  test("«Обновить раздел» and «Оформить по стандарту» (открытый файл) need an open tab", () => {
    const withoutFile = visibleSuggestions(ASSISTANT_SUGGESTIONS, NO_CONTEXT).map((s) => s.id);
    expect(withoutFile).not.toContain("update-section");
    expect(withoutFile).not.toContain("format-open-file");
    expect(withoutFile).toContain("format-to-standard");

    const withFile = visibleSuggestions(ASSISTANT_SUGGESTIONS, {
      ...NO_CONTEXT,
      activeFilePath: "operations/createSignOperation/request.adoc",
    }).map((s) => s.id);
    expect(withFile).toContain("update-section");
    expect(withFile).toContain("format-open-file");
    expect(withFile).not.toContain("format-to-standard");
  });

  test("«Проверить мои правки» needs uncommitted changes", () => {
    expect(visibleSuggestions(ASSISTANT_SUGGESTIONS, NO_CONTEXT).map((s) => s.id)).not.toContain(
      "review-doc-changes",
    );
    expect(
      visibleSuggestions(ASSISTANT_SUGGESTIONS, {
        ...NO_CONTEXT,
        hasUncommittedChanges: true,
      }).map((s) => s.id),
    ).toContain("review-doc-changes");
  });

  test("context-free suggestions always show", () => {
    const ids = visibleSuggestions(ASSISTANT_SUGGESTIONS, NO_CONTEXT).map((s) => s.id);
    expect(ids).toEqual([
      "new-method-doc",
      "format-to-standard",
      "plan-feature-docs",
      "plan-api-change",
      "plan-cleanup",
      "find-gaps",
      "sync-with-code",
      "explain-feature",
    ]);
  });
});

describe("suggestionsForMode", () => {
  test("each mode offers only the tasks it can carry out", () => {
    expect(suggestionsForMode(ASSISTANT_SUGGESTIONS, EVERYTHING).map((s) => s.id)).toEqual([
      "new-method-doc",
      "update-section",
      "describe-algorithm",
      "describe-errors",
      "format-open-file",
    ]);
    expect(
      suggestionsForMode(ASSISTANT_SUGGESTIONS, { ...EVERYTHING, conversationMode: "plan" }).map(
        (s) => s.id,
      ),
    ).toEqual(["plan-feature-docs", "plan-api-change", "plan-cleanup"]);
    expect(
      suggestionsForMode(ASSISTANT_SUGGESTIONS, {
        ...EVERYTHING,
        conversationMode: "question",
      }).map((s) => s.id),
    ).toEqual(["find-gaps", "sync-with-code", "explain-feature", "review-doc-changes"]);
  });

  test("every mode has something to offer even in the barest context", () => {
    for (const mode of ["agent", "plan", "question"] as const) {
      const ids = suggestionsForMode(ASSISTANT_SUGGESTIONS, {
        ...NO_CONTEXT,
        conversationMode: mode,
      });
      expect({ mode, empty: ids.length === 0 }).toEqual({ mode, empty: false });
    }
  });

  test("agent mode offers universal tasks without a file; file tasks when a tab is open", () => {
    expect(
      suggestionsForMode(ASSISTANT_SUGGESTIONS, {
        ...NO_CONTEXT,
        conversationMode: "agent",
      }).map((s) => s.id),
    ).toEqual(["new-method-doc", "format-to-standard"]);

    expect(
      suggestionsForMode(ASSISTANT_SUGGESTIONS, {
        ...NO_CONTEXT,
        conversationMode: "agent",
        activeFilePath: "operations/createSignOperation/request.adoc",
      }).map((s) => s.id),
    ).toEqual(["new-method-doc", "update-section", "format-open-file"]);
  });

  test("the method-only suggestions need a method description open", () => {
    // An open request.adoc is not the method doc: «Описать алгоритм» and
    // «Описать ошибки» both edit the method description itself.
    const onRequestAdoc = suggestionsForMode(ASSISTANT_SUGGESTIONS, {
      ...EVERYTHING,
      activeFilePath: "operations/createSignOperation/request.adoc",
      isMethodDoc: false,
    }).map((s) => s.id);
    expect(onRequestAdoc).not.toContain("describe-algorithm");
    expect(onRequestAdoc).not.toContain("describe-errors");
    expect(onRequestAdoc).toContain("update-section");
    expect(onRequestAdoc).toContain("format-open-file");
    expect(onRequestAdoc).not.toContain("format-to-standard");
  });

  test("Plan mode never offers a suggestion that writes files", () => {
    // Plan mode has no mutation tools at all — a writing chip there would be
    // an instruction the assistant cannot follow.
    const planning = ALL.filter((s) => s.mode === "plan");
    expect(planning.length).toBeGreaterThan(0);
    expect(planning.filter((s) => s.writes).map((s) => s.id)).toEqual([]);
  });
});

describe("buildSuggestionContext", () => {
  test("recognises a REST method description file", () => {
    expect(
      buildSuggestionContext({
        conversationMode: "agent",
        activeFilePath: "operations/createSignOperation/createSignOperation.adoc",
        hasUncommittedChanges: false,
      }).isMethodDoc,
    ).toBe(true);
  });

  test("request.adoc inside a method folder is not the method doc", () => {
    expect(
      buildSuggestionContext({
        conversationMode: "agent",
        activeFilePath: "operations/createSignOperation/request.adoc",
        hasUncommittedChanges: false,
      }).isMethodDoc,
    ).toBe(false);
  });

  test("nothing open", () => {
    expect(
      buildSuggestionContext({
        conversationMode: "question",
        activeFilePath: null,
        hasUncommittedChanges: true,
      }),
    ).toEqual({
      conversationMode: "question",
      activeFilePath: null,
      isMethodDoc: false,
      hasUncommittedChanges: true,
    });
  });
});

describe("prefillValues", () => {
  test("carries a value into a follow-up that asks for the same key", () => {
    const child: AssistantSuggestion = {
      id: "c",
      label: "c",
      mode: "agent",
      writes: true,
      text: "для метода {{method}} и {{other}}",
      inputs: [
        { key: "method", label: "m", placeholder: "" },
        { key: "other", label: "o", placeholder: "" },
      ],
    };
    expect(prefillValues(child, { method: "createSignOperationV2", unrelated: "x" })).toEqual({
      method: "createSignOperationV2",
    });
  });

  test("a suggestion without inputs is seeded with nothing", () => {
    const leaf = ALL.find((s) => s.id === "new-method-doc.response-example")!;
    expect(prefillValues(leaf, { method: "x" })).toEqual({});
  });

  test("prefills method from the parent folder of the open file", () => {
    const format = ALL.find((s) => s.id === "format-to-standard")!;
    expect(
      prefillValues(format, {}, "operations/createSignOperation/request.adoc"),
    ).toEqual({
      method: "createSignOperation",
    });
  });
});

describe("access and form gating", () => {
  const explain = ALL.find((s) => s.id === "explain-feature")!;
  const findGaps = ALL.find((s) => s.id === "find-gaps")!;

  test("needsAccessUpgrade only when widening docs-only to full repo", () => {
    expect(needsAccessUpgrade(explain, "docsOnly")).toBe(true);
    expect(needsAccessUpgrade(explain, "fullRepo")).toBe(false);
    expect(needsAccessUpgrade(findGaps, "docsOnly")).toBe(false);
  });

  test("a chip opens the form for inputs, or for the access upgrade alone", () => {
    expect(needsSuggestionForm(explain, "fullRepo")).toBe(true); // has {{feature}}
    expect(needsSuggestionForm(findGaps, "docsOnly")).toBe(false); // fills the composer directly

    const accessOnly: AssistantSuggestion = {
      id: "a",
      label: "a",
      mode: "question",
      writes: false,
      access: "fullRepo",
      text: "no inputs",
    };
    expect(needsSuggestionForm(accessOnly, "docsOnly")).toBe(true);
    expect(needsSuggestionForm(accessOnly, "fullRepo")).toBe(false);
  });

  test("required inputs gate the submit, optional ones do not", () => {
    expect(suggestionFormComplete(explain, {})).toBe(false);
    expect(suggestionFormComplete(explain, { feature: "   " })).toBe(false);
    expect(suggestionFormComplete(explain, { feature: "подпись" })).toBe(true);
    expect(suggestionFormComplete(findGaps, {})).toBe(true);
  });
});

describe("suggestionBlockedReason", () => {
  const suggestion: AssistantSuggestion = {
    id: "t",
    label: "Тест",
    text: "текст",
    mode: "agent",
    writes: false,
  };

  test("nothing is blocked while no turn is running", () => {
    expect(
      suggestionBlockedReason(suggestion, {
        sending: false,
        conversationMode: "plan",
        accessMode: "docsOnly",
      }),
    ).toBeNull();
  });

  /** Ровно тот случай, с которого начался баг: ход не закончен (пауза на
   *  `requestArtifact`), но само предложение ничего не переключает — гасить
   *  его незачем, оно только заполняет поле ввода. */
  test("a same-mode suggestion stays clickable while the turn is still open", () => {
    expect(
      suggestionBlockedReason(suggestion, {
        sending: true,
        conversationMode: "agent",
        accessMode: "docsOnly",
      }),
    ).toBeNull();
  });

  test("switching the chat mode mid-turn is blocked, with a reason", () => {
    const reason = suggestionBlockedReason(suggestion, {
      sending: true,
      conversationMode: "plan",
      accessMode: "docsOnly",
    });
    expect(reason).toContain("режим чата");
  });

  test("widening repo access mid-turn is blocked, with a reason", () => {
    const reason = suggestionBlockedReason(
      { ...suggestion, access: "fullRepo" },
      { sending: true, conversationMode: "agent", accessMode: "docsOnly" },
    );
    expect(reason).toContain("доступ");
  });

  test("an already-granted access level is not a reason to block", () => {
    expect(
      suggestionBlockedReason(
        { ...suggestion, access: "fullRepo" },
        { sending: true, conversationMode: "agent", accessMode: "fullRepo" },
      ),
    ).toBeNull();
  });
});
