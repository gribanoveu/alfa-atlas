import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

let cloning = false;
const cancelCalls: number[] = [];

mock.module("../hooks/useCloneRepo", () => ({
  useCloneRepo: () => ({
    url: "ssh://git@host/g/repo.git",
    setUrl: () => {},
    destination: "C:\\repos\\repo",
    setDestination: () => {},
    pickDestination: async () => {},
    message: null,
    cloning,
    busyLabel: cloning ? "Клонирование…" : null,
    needsAuth: false,
    conflict: false,
    stalled: false,
    submit: async () => {},
    cancel: () => cancelCalls.push(1),
    submitDisabled: cloning,
  }),
}));

const { CloneRepoModal } = await import("../components/Welcome/CloneRepoModal");

describe("CloneRepoModal", () => {
  test("cancel stays usable while a clone is running", () => {
    // The reported symptom: a hung clone left no way out but killing the app.
    cloning = true;
    cancelCalls.length = 0;
    const closes: number[] = [];
    render(<CloneRepoModal onClose={() => closes.push(1)} />);

    const cancel = screen.getByRole("button", { name: "Отмена" });
    expect((cancel as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(cancel);
    expect(cancelCalls).toHaveLength(1);
    expect(closes).toHaveLength(1);
  });

  test("closing before a clone starts does not cancel anything", () => {
    cloning = false;
    cancelCalls.length = 0;
    const closes: number[] = [];
    render(<CloneRepoModal onClose={() => closes.push(1)} />);

    fireEvent.click(screen.getByRole("button", { name: "Отмена" }));
    expect(cancelCalls).toHaveLength(0);
    expect(closes).toHaveLength(1);
  });
});
