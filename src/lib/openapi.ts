import { invoke } from "@tauri-apps/api/core";

export type SpecsRepoInfo = {
  specsRoot: string;
  entryFile: string;
  title: string | null;
  version: string | null;
};

export type RefDiagnostic = {
  pointer: string;
  ref: string;
  referencedFrom: string;
  reason: string;
};

export type OpenApiBundleResult = {
  document: Record<string, unknown>;
  diagnostics: RefDiagnostic[];
};

export function detectSpecsRepo(repoRoot: string): Promise<SpecsRepoInfo | null> {
  return invoke<SpecsRepoInfo | null>("detect_specs_repo", { repoRoot });
}

export function loadOpenApiBundle(
  repoRoot: string,
  entryFile: string,
): Promise<OpenApiBundleResult> {
  return invoke<OpenApiBundleResult>("load_openapi_bundle", { repoRoot, entryFile });
}
