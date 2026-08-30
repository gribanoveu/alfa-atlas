import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AssistantSuggestionModal } from "../components/RightDock/AssistantSuggestionModal";
import type { AssistantSuggestion } from "../lib/assistantSuggestions";

afterEach(cleanup);

const METHOD_SUGGESTION: AssistantSuggestion = {
  id: "new-method-doc",
  label: "Документация на новый метод",
  hint: "Создаёт папку с заготовками",
  mode: "agent",
  writes: true,
  text: "Заведи документацию на метод {{method}}.",
  inputs: [
    { key: "method", label: "Название метода", placeholder: "createSignOperationV2", required: true },
  ],
};

const FULL_REPO_SUGGESTION: AssistantSuggestion = {
  id: "explain-feature",
  label: "Объяснить фичу",
  mode: "question",
  writes: false,
  access: "fullRepo",
  text: "Объясни фичу: {{feature}}.",
  inputs: [{ key: "feature", label: "Фича", placeholder: "подпись", required: true }],
};

function renderModal(
  suggestion: AssistantSuggestion,
  overrides: {
    accessMode?: "docsOnly" | "fullRepo";
    initialValues?: Record<string, string>;
  } = {},
) {
  const submitted: Record<string, string>[] = [];
  let cancelled = 0;
  render(
    <AssistantSuggestionModal
      suggestion={suggestion}
      initialValues={overrides.initialValues ?? {}}
      accessMode={overrides.accessMode ?? "docsOnly"}
      onCancel={() => {
        cancelled += 1;
      }}
      onSubmit={(values) => submitted.push(values)}
    />,
  );
  return { submitted, cancelledCount: () => cancelled };
}

describe("AssistantSuggestionModal", () => {
  test("renders a labelled field per input, seeded from remembered values", () => {
    renderModal(METHOD_SUGGESTION, { initialValues: { method: "createSignOperationV2" } });

    expect(screen.getByText("Документация на новый метод")).toBeDefined();
    expect(screen.getByText("Создаёт папку с заготовками")).toBeDefined();
    const field = screen.getByPlaceholderText("createSignOperationV2") as HTMLInputElement;
    expect(field.value).toBe("createSignOperationV2");
  });

  test("submit is blocked while a required field is empty", () => {
    const { submitted } = renderModal(METHOD_SUGGESTION);

    const submit = screen.getByRole("button", { name: "Вставить в чат" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.click(submit);
    expect(submitted).toHaveLength(0);
  });

  test("hands back the typed values", () => {
    const { submitted } = renderModal(METHOD_SUGGESTION);

    fireEvent.change(screen.getByPlaceholderText("createSignOperationV2"), {
      target: { value: "createSignOperationV2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Вставить в чат" }));

    expect(submitted).toEqual([{ method: "createSignOperationV2" }]);
  });

  test("a full-repo suggestion asks for the escalation, and the submit is the consent", () => {
    const { submitted } = renderModal(FULL_REPO_SUGGESTION, { accessMode: "docsOnly" });

    expect(screen.getByRole("note").textContent).toContain("доступ ко всему репозиторию");
    fireEvent.change(screen.getByPlaceholderText("подпись"), { target: { value: "подпись" } });
    fireEvent.click(screen.getByRole("button", { name: "Включить доступ и вставить" }));

    expect(submitted).toEqual([{ feature: "подпись" }]);
  });

  test("no escalation notice once full-repo access is already on", () => {
    renderModal(FULL_REPO_SUGGESTION, { accessMode: "fullRepo" });

    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.getByRole("button", { name: "Вставить в чат" })).toBeDefined();
  });

  test("cancels on the backdrop and on the cancel button", () => {
    const { cancelledCount } = renderModal(METHOD_SUGGESTION);

    fireEvent.click(screen.getByRole("button", { name: "Отмена" }));
    expect(cancelledCount()).toBe(1);
  });
});
