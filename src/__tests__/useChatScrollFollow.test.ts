import { act, renderHook } from "@testing-library/react";
import { describe, expect, test } from "bun:test";
import type {
  UIEvent,
  WheelEvent,
} from "react";
import { useChatScrollFollow } from "../hooks/useChatScrollFollow";

function setGeometry(
  element: HTMLDivElement,
  values: {
    scrollHeight: number;
    clientHeight: number;
    scrollTop: number;
  },
) {
  Object.defineProperties(element, {
    scrollHeight: {
      configurable: true,
      value: values.scrollHeight,
    },
    clientHeight: {
      configurable: true,
      value: values.clientHeight,
    },
    scrollTop: {
      configurable: true,
      writable: true,
      value: values.scrollTop,
    },
  });
}

describe("useChatScrollFollow", () => {
  test("initial content starts at the transcript bottom", () => {
    const { result, rerender } = renderHook(
      ({ version }) => useChatScrollFollow(version, 0),
      { initialProps: { version: 0 } },
    );
    const element = document.createElement("div");
    setGeometry(element, {
      scrollHeight: 900,
      clientHeight: 300,
      scrollTop: 0,
    });
    result.current.scrollRef.current = element;

    rerender({ version: 1 });
    expect(element.scrollTop).toBe(900);
  });

  test("scrolling upward detaches follow until the bottom is reached", () => {
    const { result } = renderHook(() =>
      useChatScrollFollow("messages", 0),
    );
    const element = document.createElement("div");
    setGeometry(element, {
      scrollHeight: 900,
      clientHeight: 300,
      scrollTop: 400,
    });
    result.current.scrollRef.current = element;

    act(() => {
      result.current.onWheel({
        deltaY: -1,
      } as WheelEvent<HTMLDivElement>);
    });
    expect(result.current.showJumpToBottom).toBe(true);

    element.scrollTop = 600;
    act(() => {
      result.current.onScroll({
        currentTarget: element,
      } as UIEvent<HTMLDivElement>);
    });
    expect(result.current.showJumpToBottom).toBe(false);
  });

  test("jumping down restores follow mode", () => {
    const { result } = renderHook(() =>
      useChatScrollFollow("messages", 0),
    );
    const element = document.createElement("div");
    setGeometry(element, {
      scrollHeight: 750,
      clientHeight: 250,
      scrollTop: 100,
    });
    let target: ScrollToOptions | undefined;
    element.scrollTo = (options) => {
      target = options as ScrollToOptions;
    };
    result.current.scrollRef.current = element;

    act(() => {
      result.current.onWheel({
        deltaY: -1,
      } as WheelEvent<HTMLDivElement>);
      result.current.jumpToBottom();
    });

    expect(result.current.showJumpToBottom).toBe(false);
    expect(target).toEqual({ top: 750, behavior: "smooth" });
  });
});
