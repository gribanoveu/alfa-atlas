import { afterEach, describe, expect, mock, test } from "bun:test";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ArtifactContent } from "../lib/artifacts";

afterEach(cleanup);

const copiedText: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copiedText.push(text);
  },
}));

// The preview goes through the real Rust renderer over IPC; here we only
// need to know the builder asks for it with the current spec.
const renderCalls: ArtifactContent[] = [];
mock.module("../lib/artifacts", () => ({
  ARTIFACT_KIND_LABELS: { httpRequest: "HTTP-запрос" },
  artifactRender: async (content: ArtifactContent) => {
    renderCalls.push(content);
    return {
      kind: "httpRequest",
      inputParams: `== Входные параметры\n${content.method}`,
      outputParams: "",
      curl: "",
      responseExamples: "",
      errors: "",
      requestAdoc: "",
      responseAdoc: "",
    };
  },
}));

const { HttpRequestBuilder } = await import("../components/Artifacts/HttpRequestBuilder");

function emptySpec(): ArtifactContent {
  return {
    kind: "httpRequest",
    method: "GET",
    baseUrl: "",
    path: "",
    pathParams: [],
    queryParams: [],
    headers: [],
    body: null,
    responses: [],
    errors: [],
    notes: null,
  };
}

/** Renders the builder as a controlled component and returns the latest
 *  spec it reported — the form owns no state of its own, so this is the
 *  only way to observe an edit. */
function renderBuilder(initial: ArtifactContent = emptySpec()) {
  let latest = initial;
  const onChange = mock((next: ArtifactContent) => {
    latest = next;
  });
  const view = render(<HttpRequestBuilder spec={initial} onChange={onChange} />);
  return { view, onChange, get spec() { return latest; } };
}

describe("HttpRequestBuilder", () => {
  test("editing the request line reports the new spec", () => {
    const b = renderBuilder();
    fireEvent.change(screen.getByLabelText("Путь"), {
      target: { value: "/api/documents" },
    });
    expect(b.spec.path).toBe("/api/documents");

    // The method picker is a programmatic dropdown (trigger + option list),
    // not a native <select> — click it open, then click the option.
    fireEvent.click(screen.getByLabelText("HTTP-метод"));
    fireEvent.click(screen.getByRole("option", { name: "POST" }));
    expect(b.spec.method).toBe("POST");
  });

  test("offers to declare path placeholders that have no row yet", () => {
    const b = renderBuilder({ ...emptySpec(), path: "/api/{organizationId}/documents" });
    expect(screen.getByText(/\{organizationId\}/)).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Добавить их" }));
    expect(b.spec.pathParams.map((p) => p.name)).toEqual(["organizationId"]);
  });

  test("says nothing when every placeholder is already described", () => {
    renderBuilder({
      ...emptySpec(),
      path: "/api/{id}",
      pathParams: [{ name: "id", format: "string", required: true, description: "", values: "" }],
    });
    expect(screen.queryByRole("button", { name: "Добавить их" })).toBeNull();
  });

  test("adding a parameter row shows the five template columns", () => {
    renderBuilder();
    fireEvent.click(screen.getByRole("button", { name: /Добавить параметр/ }));
    // The row is only rendered once the parent feeds the new spec back, so
    // assert on the header, which is what fixes the column contract.
    for (const column of ["Параметр", "Формат", "Обяз.", "Описание", "Варианты значений"]) {
      expect(screen.queryByText(column)).toBeDefined();
    }
  });

  test("parameter fields are multi-line textareas, not single-line inputs", () => {
    renderBuilder({
      ...emptySpec(),
      pathParams: [{ name: "id", format: "string", required: true, description: "", values: "" }],
    });
    // A plain <input> can never wrap or grow — this is the whole point of
    // the change, so pin the actual tag rather than just behavior.
    expect(screen.getByLabelText("Описание параметра 1").tagName).toBe("TEXTAREA");
  });

  test("a long description grows the field's height instead of clipping it", () => {
    // `renderBuilder`'s `onChange` mock only records the latest spec — it
    // never feeds it back in, which every other test here relies on to
    // stay simple. This one genuinely needs the round trip (the resize
    // effect keys off the *prop* changing), so it renders through a small
    // stateful wrapper instead, the same shape `ArtifactView` uses in the
    // real app.
    function Controlled() {
      const [spec, setSpec] = useState<ArtifactContent>({
        ...emptySpec(),
        pathParams: [{ name: "id", format: "string", required: true, description: "", values: "" }],
      });
      return <HttpRequestBuilder spec={spec} onChange={setSpec} />;
    }
    render(<Controlled />);
    const field = screen.getByLabelText("Описание параметра 1") as HTMLTextAreaElement;

    // happy-dom has no real layout engine, so `scrollHeight` reports a
    // fixed 0 regardless of content — overriding it is the standard way to
    // exercise a textarea-autosize effect deterministically: force what a
    // real browser would report once the value wraps to three lines, and
    // assert AutoGrowField actually applies that measurement, not that a
    // real layout engine produced it.
    Object.defineProperty(field, "scrollHeight", { configurable: true, value: 84 });

    fireEvent.change(field, {
      target: {
        value:
          "Очень длинное описание параметра, которое не должно помещаться в одну строку конструктора запроса.",
      },
    });

    expect(field.style.height).toBe("84px");
  });

  test("«Разобрать поля из JSON» seeds body rows from the example", () => {
    const b = renderBuilder({
      ...emptySpec(),
      body: {
        mediaType: "application/json",
        sample: '{"type":"INVOICE","sum":100}',
        params: [],
      },
    });
    fireEvent.click(screen.getByRole("tab", { name: /Тело/ }));
    fireEvent.click(screen.getByRole("button", { name: /Разобрать поля из JSON/ }));

    expect(b.spec.body?.params.map((p) => [p.name, p.format, p.values])).toEqual([
      ["type", "string", "INVOICE"],
      ["sum", "integer", "100"],
    ]);
  });

  test("a request with no body offers to add one rather than showing an empty table", () => {
    const b = renderBuilder();
    fireEvent.click(screen.getByRole("tab", { name: /Тело/ }));
    expect(screen.getByText("У запроса нет тела.")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /Добавить тело запроса/ }));
    expect(b.spec.body).toEqual({ mediaType: "application/json", sample: "", params: [] });
  });

  test("the preview renders through the shared renderer, not a local copy", async () => {
    renderBuilder({ ...emptySpec(), method: "PATCH" });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 300));
    });
    await waitFor(() => expect(renderCalls.length).toBeGreaterThan(0));
    expect(renderCalls[renderCalls.length - 1]!.method).toBe("PATCH");
    // The rendered preview lives behind the "Результат" tab now.
    fireEvent.click(screen.getByRole("tab", { name: "Результат" }));
    expect(screen.getByText(/== Входные параметры/)).toBeDefined();
  });

  test("opens on the constructor by default", () => {
    renderBuilder();
    expect(screen.getByRole("tab", { name: "Конструктор" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tab", { name: "Результат" }).getAttribute("aria-selected")).toBe("false");
    expect(screen.getByLabelText("Путь")).toBeDefined();
  });

  test("switching to Результат hides the form and shows the preview tabs", () => {
    renderBuilder();
    fireEvent.click(screen.getByRole("tab", { name: "Результат" }));
    expect(screen.queryByLabelText("Путь")).toBeNull();
    expect(screen.getByRole("tab", { name: "curl" })).toBeDefined();
  });

  test("копирование результата копирует текст активной вкладки предпросмотра", async () => {
    copiedText.length = 0;
    renderBuilder({ ...emptySpec(), method: "PUT" });
    fireEvent.click(screen.getByRole("tab", { name: "Результат" }));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 300));
    });
    await waitFor(() => expect(screen.getByText(/== Входные параметры/)).toBeDefined());

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать результат"));
    });

    expect(copiedText).toEqual(["== Входные параметры\nPUT"]);
  });

  test("копировать нечего, пока предпросмотр ещё не пришёл", () => {
    renderBuilder();
    fireEvent.click(screen.getByRole("tab", { name: "Результат" }));
    expect((screen.getByLabelText("Скопировать результат") as HTMLButtonElement).disabled).toBe(true);
  });
});
