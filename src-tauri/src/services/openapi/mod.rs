pub mod bundle;
pub mod detection;
pub mod gradle;

pub use bundle::{is_common_spec_fallback_path, load_openapi_bundle};
pub use detection::{detect_specs_repo, score_specs_signals, KNOWN_SUBDIRS};

#[cfg(test)]
mod tests;
