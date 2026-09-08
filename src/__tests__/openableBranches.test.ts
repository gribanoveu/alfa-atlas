import { describe, expect, test } from "bun:test";
import { localBranchName, openableBranches, type GitBranchInfo } from "../lib/git";

function branch(
  name: string,
  extra: Partial<GitBranchInfo> = {},
): GitBranchInfo {
  return {
    name,
    isCurrent: false,
    isRemote: false,
    behind: null,
    tipOid: null,
    ...extra,
  };
}

describe("localBranchName", () => {
  test("strips only the remote name", () => {
    expect(localBranchName(branch("origin/doc/template", { isRemote: true }))).toBe(
      "doc/template",
    );
    expect(localBranchName(branch("doc/template"))).toBe("doc/template");
  });
});

describe("openableBranches", () => {
  test("freshly cloned repo offers the remote-only template branch", () => {
    const offered = openableBranches([
      branch("master", { isCurrent: true }),
      branch("origin/master", { isRemote: true }),
      branch("origin/doc/template", { isRemote: true }),
    ]);
    expect(offered.map((b) => b.name)).toEqual(["master", "origin/doc/template"]);
  });

  test("current branch first, rest alphabetical by local name", () => {
    const offered = openableBranches([
      branch("zeta"),
      branch("alpha"),
      branch("main", { isCurrent: true }),
    ]);
    expect(offered.map((b) => b.name)).toEqual(["main", "alpha", "zeta"]);
  });
});
