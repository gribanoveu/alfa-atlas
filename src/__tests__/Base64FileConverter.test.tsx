import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import * as actualClipboard from "../lib/clipboard";
import * as actualFileSave from "../lib/fileSave";

afterEach(cleanup);

// Same process-wide caveat as `fileSave` below: `readClipboardText` has to
// survive, or the edit menu's paste path loses its binding.
mock.module("../lib/clipboard", () => ({
  ...actualClipboard,
  copyToClipboard: async () => {},
}));

// `mock.module` is process-wide and replaces the whole module: keep the
// rest of `fileSave`'s surface, or another test file importing
// `saveBytesViaDialog` from it fails to resolve the binding.
mock.module("../lib/fileSave", () => ({
  ...actualFileSave,
  saveDecodedBinaryFile: async () => false,
}));

const TINY_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAD0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const { Base64FileConverter } = await import("../components/Utilities/Base64FileConverter");

describe("Base64FileConverter", () => {
  test("decode показывает предпросмотр изображения", () => {
    render(<Base64FileConverter />);
    fireEvent.change(screen.getByLabelText("Base64 или data URI"), {
      target: { value: TINY_PNG_BASE64 },
    });

    expect(screen.getByText("Предпросмотр")).toBeDefined();
    expect(screen.getByAltText("Предпросмотр изображения")).toBeDefined();
    expect(screen.getByRole("button", { name: "Сохранить файл" })).toBeDefined();
  });

  test("decode data URI определяет тип", () => {
    render(<Base64FileConverter />);
    fireEvent.change(screen.getByLabelText("Base64 или data URI"), {
      target: { value: `data:image/png;base64,${TINY_PNG_BASE64}` },
    });

    expect(screen.getByText("PNG")).toBeDefined();
  });

  test("encode через выбор файла показывает Base64", async () => {
    render(<Base64FileConverter />);
    fireEvent.click(screen.getByRole("tab", { name: "Файл → Base64" }));

    const input = document.querySelector('input[type="file"]') as HTMLInputElement;
    const pngBytes = Uint8Array.from(atob(TINY_PNG_BASE64), (char) => char.charCodeAt(0));
    const file = new File([pngBytes], "pixel.png", { type: "image/png" });

    await act(async () => {
      fireEvent.change(input, { target: { files: [file] } });
    });

    expect(screen.getByLabelText("Результат Base64").textContent).toContain(TINY_PNG_BASE64);
    expect(screen.getByText(/pixel\.png/)).toBeDefined();
  });
});
