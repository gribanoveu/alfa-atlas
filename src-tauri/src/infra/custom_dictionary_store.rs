use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::domain::spellcheck::SpellcheckError;

const DICTIONARIES_DIR_NAME: &str = "dictionaries";
const CUSTOM_DICTIONARY_FILE_NAME: &str = "custom.txt";

fn dictionaries_dir() -> Result<PathBuf, SpellcheckError> {
    let home = dirs::home_dir().ok_or(SpellcheckError::HomeDirUnavailable)?;
    Ok(home.join(".atlas").join(DICTIONARIES_DIR_NAME))
}

fn custom_dictionary_path() -> Result<PathBuf, SpellcheckError> {
    Ok(dictionaries_dir()?.join(CUSTOM_DICTIONARY_FILE_NAME))
}

/// Loads the personal word list from `~/.atlas/dictionaries/custom.txt`
/// (one word per line). Missing file yields an empty list.
pub fn load_custom_words() -> Result<Vec<String>, SpellcheckError> {
    let path = custom_dictionary_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path).map_err(SpellcheckError::Read)?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// Persists the personal word list, sorted and deduplicated for a stable,
/// diffable, hand-editable plain-text file.
pub fn save_custom_words(words: &BTreeSet<String>) -> Result<(), SpellcheckError> {
    let dir = dictionaries_dir()?;
    fs::create_dir_all(&dir).map_err(SpellcheckError::CreateDir)?;

    let path = dir.join(CUSTOM_DICTIONARY_FILE_NAME);
    let contents = words
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, contents).map_err(SpellcheckError::Write)?;
    Ok(())
}
