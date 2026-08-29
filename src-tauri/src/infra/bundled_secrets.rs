//! Compile-time secrets injected by `build.rs` from `EMBEDDING_API_KEY` or
//! `.secrets/embedding_api_key` — never committed to git.

include!(concat!(env!("OUT_DIR"), "/bundled_secrets.rs"));
