use crate::domain::embeddings::EmbeddingProviderConfig;
use crate::domain::settings::SettingsError;
use crate::infra::settings_store;

pub fn load_embedding_config() -> Result<EmbeddingProviderConfig, SettingsError> {
    Ok(settings_store::load()?.embedding)
}

pub fn save_embedding_config(config: EmbeddingProviderConfig) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.embedding = config;
    settings_store::save(&settings)
}
