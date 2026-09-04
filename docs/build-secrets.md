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

## Where the master key lives

Every `*.enc` file under `~/.atlas` is sealed with one AES-256 master key held in the **OS keychain** (macOS Keychain, Windows Credential Manager, Linux Secret Service) — see `infra::master_key`. `~/.atlas/.enc_key` is a fallback for machines with no reachable keychain; when a keychain becomes available the key is moved into it and the file is shredded, keeping the same key bytes so existing blobs stay readable.

`~/.atlas/master_key_store.json` records *which* of the two currently holds the key (no key material). Without it, an unreachable keychain — locked, prompt dismissed, Secret Service not up yet — is indistinguishable from a first run, and minting a key at that moment would permanently orphan every sealed blob. With it, the app reports the keychain as unavailable and leaves the credentials alone until it is back.

Two things to know when changing this:

- **The `keyring` dependency needs its per-target `features`.** Platform backends are opt-in in keyring 3.x, and without them the crate silently compiles to an in-memory mock whose writes vanish at process exit — every secret would quietly fall back to the plaintext key file. `master_key` detects this at runtime, and `native_keychain_backend_is_compiled_in` fails the build if the features go missing.
- **The keychain does not defend against malware running as the user.** On all three platforms an unlocked keychain is readable by that user's processes; what it buys is that the key no longer travels with a copy of `~/.atlas` (backups, cloud-synced home directories, support bundles). Defending against same-user code needs a master password, which does not exist yet.

Blobs carry a `"ATLS" | version | purpose` header that is also the AES-GCM AAD, so a blob can only be opened as the kind of secret it was written as. Pre-header blobs are still readable and are rewritten in the current format on first read.

## TLS certificates in manifest

CA root certificates in `system_providers.yaml` are **not** secrets — public trust anchors. Use YAML multiline `trustedCertPem: |` blocks for readable PEM.
