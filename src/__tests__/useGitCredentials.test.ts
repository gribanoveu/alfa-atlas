import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { GitCredentials, SshKeyConfig } from "../lib/git";
import * as actualGit from "../lib/git";

let stored: GitCredentials;
let keyStatus = { exists: true, publicKey: "ssh-ed25519 AAAA..." };
let saveFails: string | null = null;
let saves: GitCredentials[] = [];
let genFails: string | null = null;
let pickResult: string | null = null;

mock.module("../lib/git", () => ({
  ...actualGit,
  gitGetCredentials: async () => stored,
  gitGetKeyStatus: async () => keyStatus,
  gitSaveCredentials: async (c: GitCredentials) => {
    if (saveFails) throw saveFails;
    saves.push(c);
    stored = c;
  },
  gitGenerateKey: async () => {
    if (genFails) throw genFails;
    return { exists: true, publicKey: "ssh-ed25519 NEW..." };
  },
  gitImportKey: async () => ({ exists: true, publicKey: "ssh-ed25519 IMPORTED..." }),
}));
mock.module("@tauri-apps/plugin-dialog", () => ({ open: async () => pickResult }));

const { useGitCredentials } = await import("../hooks/useGitCredentials");

function key(name: string): SshKeyConfig {
  return { host: name, keyPath: `/keys/${name}` } as SshKeyConfig;
}

beforeEach(() => {
  stored = { sshKeys: [key("a")], trustAllSshHostKeys: false } as GitCredentials;
  keyStatus = { exists: true, publicKey: "ssh-ed25519 AAAA..." };
  saveFails = null;
  saves = [];
  genFails = null;
  pickResult = null;
});

describe("useGitCredentials", () => {
  test("loads credentials and key status together", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());
    expect(result.current.keyStatus?.exists).toBe(true);
  });

  test("toggling trust-all writes through", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      result.current.toggleTrustAll();
    });

    expect(saves.at(-1)?.trustAllSshHostKeys).toBe(true);
    expect(result.current.credentials?.trustAllSshHostKeys).toBe(true);
  });

  test("a failed write rolls back to what the backend holds", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());
    saveFails = "keychain locked";

    await act(async () => {
      result.current.toggleTrustAll();
    });

    expect(result.current.credentials?.trustAllSshHostKeys).toBe(false);
    expect(result.current.error).toBe("keychain locked");
    expect(result.current.busy).toBe(false);
  });

  test("saving with a null index appends a key", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      result.current.saveKey(key("b"), null);
    });

    expect(saves.at(-1)?.sshKeys.map((k) => k.host)).toEqual(["a", "b"]);
  });

  test("saving with an index replaces that row", async () => {
    stored = { sshKeys: [key("a"), key("b")], trustAllSshHostKeys: false } as GitCredentials;
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      result.current.saveKey(key("z"), 1);
    });

    expect(saves.at(-1)?.sshKeys.map((k) => k.host)).toEqual(["a", "z"]);
  });

  test("deleting removes exactly that row", async () => {
    stored = {
      sshKeys: [key("a"), key("b"), key("c")],
      trustAllSshHostKeys: false,
    } as GitCredentials;
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      result.current.deleteKey(1);
    });

    expect(saves.at(-1)?.sshKeys.map((k) => k.host)).toEqual(["a", "c"]);
  });

  test("generating a key replaces the status", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      await result.current.generateKey();
    });

    expect(result.current.keyStatus?.publicKey).toContain("NEW");
    expect(result.current.keyGenBusy).toBe(false);
  });

  test("a failed generation reports why and leaves the old key", async () => {
    genFails = "no entropy";
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      await result.current.generateKey();
    });

    expect(result.current.error).toBe("no entropy");
    expect(result.current.keyStatus?.publicKey).toContain("AAAA");
  });

  test("cancelling the import dialog changes nothing", async () => {
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      await result.current.importKey();
    });

    expect(result.current.keyStatus?.publicKey).toContain("AAAA");
    expect(result.current.error).toBeNull();
  });

  test("importing a picked file replaces the key", async () => {
    pickResult = "/home/u/.ssh/id_ed25519";
    const { result } = renderHook(() => useGitCredentials());
    await waitFor(() => expect(result.current.credentials).not.toBeNull());

    await act(async () => {
      await result.current.importKey();
    });

    expect(result.current.keyStatus?.publicKey).toContain("IMPORTED");
  });
});
