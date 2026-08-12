use std::sync::Arc;

use crate::domain::spellcheck::{
    BUILTIN_DICTIONARIES, DictionaryDef, DocKind, SpellIssue, SpellcheckConfig,
};
use crate::services::spellcheck::SpellcheckEngine;
use crate::services::spellcheck_prefs;

#[tauri::command]
pub fn get_dictionaries() -> Vec<DictionaryDef> {
    BUILTIN_DICTIONARIES.to_vec()
}

#[tauri::command]
pub fn get_spellcheck_config() -> Result<SpellcheckConfig, String> {
    spellcheck_prefs::load_spellcheck_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_spellcheck_config(config: SpellcheckConfig) -> Result<(), String> {
    spellcheck_prefs::save_spellcheck_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_spelling(
    engine: tauri::State<'_, Arc<SpellcheckEngine>>,
    text: String,
    doc_kind: DocKind,
    path: String,
) -> Result<Vec<SpellIssue>, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = spellcheck_prefs::load_spellcheck_config().unwrap_or_default();
        if !config.should_check_path(&path) {
            return Vec::new();
        }
        engine.check_text(&text, doc_kind, &config)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn suggest_spelling(
    engine: tauri::State<'_, Arc<SpellcheckEngine>>,
    word: String,
) -> Result<Vec<String>, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = spellcheck_prefs::load_spellcheck_config().unwrap_or_default();
        engine.suggest(&word, &config)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_custom_dictionary_words(engine: tauri::State<'_, Arc<SpellcheckEngine>>) -> Vec<String> {
    engine.custom_words()
}

#[tauri::command]
pub fn add_custom_dictionary_word(
    engine: tauri::State<'_, Arc<SpellcheckEngine>>,
    word: String,
) -> Result<(), String> {
    engine.add_custom_word(word).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_custom_dictionary_word(
    engine: tauri::State<'_, Arc<SpellcheckEngine>>,
    word: String,
) -> Result<(), String> {
    engine.remove_custom_word(&word).map_err(|e| e.to_string())
}
