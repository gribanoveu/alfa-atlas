import { describe, expect, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useAppToasts } from "../hooks/useAppToasts";

describe("useAppToasts", () => {
  test("no source and no success means no toast", () => {
    const { result } = renderHook(() => useAppToasts(null));
    expect(result.current.toast).toBeNull();
  });

  test("an error from the source shows as an error toast", () => {
    const { result } = renderHook(() => useAppToasts("не удалось сохранить"));
    expect(result.current.toast?.variant).toBe("error");
    expect(result.current.toast?.message).toBe("не удалось сохранить");
  });

  test("dismissing an error keeps it dismissed while the source still holds it", () => {
    // The whole reason this hook tracks *which* message was dismissed: the
    // error source (`editor.error`) stays set after a close, so a plain
    // boolean would let the same string pop straight back on re-render.
    const { result, rerender } = renderHook(({ e }) => useAppToasts(e), {
      initialProps: { e: "boom" as string | null },
    });
    act(() => result.current.toast?.onClose());
    expect(result.current.toast).toBeNull();

    rerender({ e: "boom" });
    expect(result.current.toast).toBeNull();
  });

  test("a different error after a dismissal shows again", () => {
    const { result, rerender } = renderHook(({ e }) => useAppToasts(e), {
      initialProps: { e: "first" as string | null },
    });
    act(() => result.current.toast?.onClose());

    rerender({ e: "second" });
    expect(result.current.toast?.message).toBe("second");
  });

  test("a success outranks an error that is still showing", () => {
    // The error may be stale; a success always reports something the user
    // just did.
    const { result } = renderHook(() => useAppToasts("старая ошибка"));
    expect(result.current.toast?.variant).toBe("error");

    act(() => result.current.showSuccess("Готово"));
    expect(result.current.toast?.variant).toBe("success");
    expect(result.current.toast?.message).toBe("Готово");
  });

  test("closing a success falls back to the error underneath", () => {
    const { result } = renderHook(() => useAppToasts("ошибка"));
    act(() => result.current.showSuccess("Готово"));
    act(() => result.current.toast?.onClose());
    expect(result.current.toast?.variant).toBe("error");
  });

  test("a second success replaces the first rather than queueing", () => {
    const { result } = renderHook(() => useAppToasts(null));
    act(() => result.current.showSuccess("Первое"));
    act(() => result.current.showSuccess("Второе"));
    expect(result.current.toast?.message).toBe("Второе");
  });

  test("folderError feeds the same slot as the source error", () => {
    const { result } = renderHook(() => useAppToasts(null));
    expect(result.current.toast).toBeNull();

    act(() => result.current.setFolderError("папка не открылась"));
    expect(result.current.toast?.variant).toBe("error");
    expect(result.current.toast?.message).toBe("папка не открылась");
  });
});
