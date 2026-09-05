import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type UIEvent,
  type WheelEvent,
} from "react";

const SCROLL_BOTTOM_THRESHOLD_PX = 4;

/**
 * Keeps a streaming transcript pinned until the user deliberately scrolls
 * upward. The hook owns only viewport mechanics; callers decide when new
 * content arrived and when sending a fresh message should re-enable follow.
 */
export function useChatScrollFollow(contentVersion: unknown, observedChildCount: number) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedToBottomRef = useRef(true);
  const didMountScrollRef = useRef(false);
  const followFrameRef = useRef<number | null>(null);
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);

  const scheduleFollowToBottom = useCallback(() => {
    if (!pinnedToBottomRef.current || followFrameRef.current !== null) return;
    followFrameRef.current = requestAnimationFrame(() => {
      followFrameRef.current = null;
      const el = scrollRef.current;
      if (!el || !pinnedToBottomRef.current) return;
      el.scrollTop = el.scrollHeight;
    });
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (!didMountScrollRef.current) {
      el.scrollTop = el.scrollHeight;
      didMountScrollRef.current = true;
      return;
    }
    scheduleFollowToBottom();
  }, [contentVersion, scheduleFollowToBottom]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(scheduleFollowToBottom);
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
    return () => observer.disconnect();
  }, [observedChildCount, scheduleFollowToBottom]);

  useEffect(
    () => () => {
      if (followFrameRef.current !== null) {
        cancelAnimationFrame(followFrameRef.current);
      }
    },
    [],
  );

  const onScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const atBottom =
      el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_BOTTOM_THRESHOLD_PX;
    if (atBottom) {
      pinnedToBottomRef.current = true;
      setShowJumpToBottom(false);
    }
  }, []);

  const onWheel = useCallback((event: WheelEvent<HTMLDivElement>) => {
    if (event.deltaY > 0) {
      setShowJumpToBottom(false);
      return;
    }
    if (event.deltaY === 0) return;
    pinnedToBottomRef.current = false;
    setShowJumpToBottom(true);
    if (followFrameRef.current !== null) {
      cancelAnimationFrame(followFrameRef.current);
      followFrameRef.current = null;
    }
  }, []);

  const jumpToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    pinnedToBottomRef.current = true;
    setShowJumpToBottom(false);
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, []);

  const followNextResponse = useCallback(() => {
    pinnedToBottomRef.current = true;
    setShowJumpToBottom(false);
  }, []);

  return {
    scrollRef,
    showJumpToBottom,
    onScroll,
    onWheel,
    jumpToBottom,
    followNextResponse,
  };
}
