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

/** Откуда в собранный документ попал узел по адресу `pointer` — пишется на
 * каждой границе `$ref`. Источник произвольного узла ищется по самому
 * длинному `pointer`-префиксу, см. `sourceForPointer`. */
export type SourceRef = {
  pointer: string;
  /** Путь относительно корня репозитория. */
  file: string;
  /** JSON Pointer внутри файла; пустой — ссылка на файл целиком. */
  fragment: string;
};

export type OpenApiBundleResult = {
  document: Record<string, unknown>;
  diagnostics: RefDiagnostic[];
  sources: SourceRef[];
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
