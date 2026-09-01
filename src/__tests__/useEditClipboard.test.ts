import { afterEach, describe, expect, test } from "bun:test";
import { renderHook } from "@testing-library/react";
import { createRef } from "react";
import type { editor as MonacoEditor } from "monaco-editor";
import { useEditClipboard } from "../hooks/useEditClipboard";

/** Focus is what the hook listens to, and happy-dom's `focus()` fires
 *  `focusin` for real — no synthetic event needed. */
function field(value: string, selection: [number, number]): HTMLInputElement {
  const el = document.createElement("input");
  el.type = "text";
  el.value = value;
  document.body.append(el);
  el.focus();
  el.setSelectionRange(...selection);
  return el;
}

function menuButton(): HTMLButtonElement {
  const nav = document.createElement("nav");
  nav.className = "menu";
  const button = document.createElement("button");
  nav.append(button);
  document.body.append(nav);
  return button;
}

const noEditor = createRef<MonacoEditor.IStandaloneCodeEditor>();

afterEach(() => {
  document.body.innerHTML = "";
});

describe("useEditClipboard", () => {
  test("без фокуса ничего не доступно", () => {
    const { result } = renderHook(() => useEditClipboard(noEditor));

    expect(result.current.availability()).toEqual({ cut: false, copy: false, paste: false });
  });

  test("видит выделение в сфокусированном поле", () => {
    const { result } = renderHook(() => useEditClipboard(noEditor));
    field("привет", [0, 3]);

    expect(result.current.availability()).toEqual({ cut: true, copy: true, paste: true });
  });

  test("фокус на самом меню не сбрасывает цель", () => {
    const { result } = renderHook(() => useEditClipboard(noEditor));
    field("привет", [0, 3]);

    menuButton().focus();

    expect(result.current.availability()).toEqual({ cut: true, copy: true, paste: true });
  });

  test("фокус вне поля цель сбрасывает", () => {
    const { result } = renderHook(() => useEditClipboard(noEditor));
    field("привет", [0, 3]);

    const other = document.createElement("button");
    document.body.append(other);
    other.focus();

    expect(result.current.availability()).toEqual({ cut: false, copy: false, paste: false });
  });

  test("исчезнувшее поле перестаёт быть целью", () => {
    const { result } = renderHook(() => useEditClipboard(noEditor));
    const el = field("привет", [0, 3]);

    el.remove();

    expect(result.current.availability()).toEqual({ cut: false, copy: false, paste: false });
  });
});
