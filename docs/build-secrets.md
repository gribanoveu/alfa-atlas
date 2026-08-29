# Build-time secrets (maintainers only)

This document is for people who **build** Alfa Atlas — not for end users. Release builds can ship a pre-configured embedding API key so users never enter one in Settings.

## Embedding API key

The remote embedding provider key is **never** stored in `system_providers.yaml` or any committed file.

At compile time, `build.rs` reads the key from (first match wins):

1. Environment variable `EMBEDDING_API_KEY`
2. Gitignored file `src-tauri/.secrets/embedding_api_key` (single line, no trailing newline required)

If neither is set, the build has no bundled key — local/dev behavior unchanged (user enters key in Settings or uses local BGE-M3).

### CI (GitHub Actions)

Add repository secret `EMBEDDING_API_KEY`. The release workflow passes it to `bun run tauri build` automatically.

### Local release build

```bash
mkdir -p src-tauri/.secrets
printf '%s' 'your-api-key-here' > src-tauri/.secrets/embedding_api_key
bun run tauri build
```

Or:

```bash
EMBEDDING_API_KEY='your-api-key-here' bun run tauri build
```

### Runtime behavior

- Bundled key is embedded in the binary (extractable with reverse engineering — acceptable for internal distribution).
- User override in Settings (`~/.atlas/embedding_credentials.enc`) takes priority over the bundled key (useful for development).
- Settings UI hides manual key entry when `apiKeyBundled` is true.

## LLM API keys

LLM provider keys are **not** injected at build time. Users set them in Settings; they are stored encrypted in `~/.atlas/llm_credentials.enc`.

## TLS certificates in manifest

CA root certificates in `system_providers.yaml` are **not** secrets — public trust anchors. Use YAML multiline `trustedCertPem: |` blocks for readable PEM.
