//! On-disk user Agent Skills under `~/.atlas/skills/<name>/SKILL.md`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::domain::agent_skills::{parse_skill_md, ParsedSkill, SkillError, SkillMeta, SkillSource};
use crate::domain::settings::SettingsError;
use crate::infra::settings_store;

const SKILLS_DIR_NAME: &str = "skills";
const SKILL_MD: &str = "SKILL.md";

pub fn user_skills_dir() -> Result<PathBuf, SkillError> {
    let home = settings_store::settings_dir().map_err(map_settings)?;
    Ok(home.join(SKILLS_DIR_NAME))
}

pub fn ensure_user_skills_dir() -> Result<PathBuf, SkillError> {
    let dir = user_skills_dir()?;
    fs::create_dir_all(&dir).map_err(|e| SkillError::Message(e.to_string()))?;
    Ok(dir)
}

pub struct UserSkillEntry {
    pub dir_name: String,
    pub parsed: Result<ParsedSkill, SkillError>,
}

/// Immediate child directories of `~/.atlas/skills`. Missing dir → empty.
pub fn scan_user_skills() -> Result<Vec<UserSkillEntry>, SkillError> {
    let dir = user_skills_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let read = fs::read_dir(&dir).map_err(|e| SkillError::Message(e.to_string()))?;
    for entry in read {
        let entry = entry.map_err(|e| SkillError::Message(e.to_string()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if dir_name.starts_with('.') {
            continue;
        }
        let skill_md = path.join(SKILL_MD);
        let parsed = match fs::read_to_string(&skill_md) {
            Ok(contents) => parse_skill_md(&contents, dir_name),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SkillError::Message(
                format!("{SKILL_MD} not found in {dir_name}"),
            )),
            Err(e) => Err(SkillError::Message(e.to_string())),
        };
        entries.push(UserSkillEntry {
            dir_name: dir_name.to_string(),
            parsed,
        });
    }
    entries.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(entries)
}

pub fn valid_user_metas() -> Result<Vec<SkillMeta>, SkillError> {
    Ok(scan_user_skills()?
        .into_iter()
        .filter_map(|e| {
            e.parsed.ok().map(|p| SkillMeta {
                name: p.name,
                description: p.description,
                source: SkillSource::User,
            })
        })
        .collect())
}

pub fn load_user_skill(name: &str) -> Result<(ParsedSkill, PathBuf), SkillError> {
    let root = skill_root(name)?;
    let contents = fs::read_to_string(root.join(SKILL_MD)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SkillError::NotFound(name.to_string())
        } else {
            SkillError::Message(e.to_string())
        }
    })?;
    let parsed = parse_skill_md(&contents, name)?;
    Ok((parsed, root))
}

pub fn companion_files(name: &str) -> Result<Vec<String>, SkillError> {
    let root = skill_root(name)?;
    if !root.is_dir() {
        return Err(SkillError::NotFound(name.to_string()));
    }
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn read_companion(name: &str, relative: &str) -> Result<String, SkillError> {
    let root = skill_root(name)?;
    let target = resolve_under(&root, relative)?;
    read_text(&target, &format!("{name}/{relative}"))
}

fn read_text(path: &Path, label: &str) -> Result<String, SkillError> {
    fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => SkillError::NotFound(label.to_string()),
        std::io::ErrorKind::InvalidData => {
            SkillError::Message(format!("{label} is not UTF-8 text"))
        }
        _ => SkillError::Message(e.to_string()),
    })
}

/// Every file in a user skill folder, `SKILL.md` first. Unlike
/// `companion_files` this resolves the folder by its directory name without
/// the spec name check, so a folder whose `SKILL.md` is broken can still be
/// previewed — reading it is how you find out what's wrong with it.
pub fn preview_files(dir_name: &str) -> Result<Vec<String>, SkillError> {
    let root = preview_root(dir_name)?;
    if !root.is_dir() {
        return Err(SkillError::NotFound(dir_name.to_string()));
    }
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort();
    if root.join(SKILL_MD).is_file() {
        files.insert(0, SKILL_MD.to_string());
    }
    Ok(files)
}

/// Text of one file inside a user skill folder — `SKILL.md` included, and
/// without the spec name check, for the same reason as `preview_files`.
pub fn preview_file(dir_name: &str, relative: &str) -> Result<String, SkillError> {
    let root = preview_root(dir_name)?;
    let target = resolve_under(&root, relative)?;
    read_text(&target, &format!("{dir_name}/{relative}"))
}

/// Copy `src_dir` into `~/.atlas/skills/{parsed.name}/`. Fails if that name
/// already exists.
pub fn import_skill_dir(src_dir: &Path) -> Result<SkillMeta, SkillError> {
    let src_dir = src_dir
        .canonicalize()
        .map_err(|e| SkillError::Message(e.to_string()))?;
    if !src_dir.is_dir() {
        return Err(SkillError::Message(format!(
            "not a directory: {}",
            src_dir.display()
        )));
    }
    let dir_name = src_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SkillError::Message("skill directory name is not valid UTF-8".into()))?;
    let contents = fs::read_to_string(src_dir.join(SKILL_MD)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SkillError::Message(format!("{SKILL_MD} not found in {}", src_dir.display()))
        } else {
            SkillError::Message(e.to_string())
        }
    })?;
    let parsed = parse_skill_md(&contents, dir_name)?;
    let dest_parent = ensure_user_skills_dir()?;
    let dest = dest_parent.join(&parsed.name);
    if dest.exists() {
        return Err(SkillError::AlreadyExists(parsed.name));
    }
    copy_dir_all(&src_dir, &dest)?;
    Ok(SkillMeta {
        name: parsed.name,
        description: parsed.description,
        source: SkillSource::User,
    })
}

pub fn remove_user_skill(name: &str) -> Result<(), SkillError> {
    crate::domain::agent_skills::validate_skill_name(name)?;
    let root = skill_root(name)?;
    if !root.exists() {
        return Err(SkillError::NotFound(name.to_string()));
    }
    fs::remove_dir_all(&root).map_err(|e| SkillError::Message(e.to_string()))
}

fn skill_root(name: &str) -> Result<PathBuf, SkillError> {
    crate::domain::agent_skills::validate_skill_name(name)?;
    Ok(user_skills_dir()?.join(name))
}

/// A skill folder resolved by directory name only. `resolve_under` keeps the
/// result inside `~/.atlas/skills`, which is what `skill_root` relies on
/// `validate_skill_name` for.
fn preview_root(dir_name: &str) -> Result<PathBuf, SkillError> {
    let dir = ensure_user_skills_dir()?;
    resolve_under(&dir, dir_name)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), SkillError> {
    let read = fs::read_dir(dir).map_err(|e| SkillError::Message(e.to_string()))?;
    for entry in read {
        let entry = entry.map_err(|e| SkillError::Message(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| SkillError::PathEscape(path.display().to_string()))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel == SKILL_MD {
            continue;
        }
        out.push(rel);
    }
    Ok(())
}

/// Resolve `relative` under `root` without leaving it. Rejects `..` and
/// absolute components before touching the disk.
pub fn resolve_under(root: &Path, relative: &str) -> Result<PathBuf, SkillError> {
    let rel = Path::new(relative);
    if rel.is_absolute() {
        return Err(SkillError::PathEscape(relative.to_string()));
    }
    for component in rel.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => return Err(SkillError::PathEscape(relative.to_string())),
        }
    }
    let joined = root.join(rel);
    let root_canon = root
        .canonicalize()
        .map_err(|e| SkillError::Message(e.to_string()))?;
    match joined.canonicalize() {
        Ok(canon) => {
            if !canon.starts_with(&root_canon) {
                return Err(SkillError::PathEscape(relative.to_string()));
            }
            Ok(canon)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Still reject if the *logical* join escaped; otherwise let the
            // caller surface NotFound.
            if !joined.starts_with(root) {
                return Err(SkillError::PathEscape(relative.to_string()));
            }
            Err(SkillError::NotFound(relative.to_string()))
        }
        Err(e) => Err(SkillError::Message(e.to_string())),
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), SkillError> {
    fs::create_dir_all(dst).map_err(|e| SkillError::Message(e.to_string()))?;
    let read = fs::read_dir(src).map_err(|e| SkillError::Message(e.to_string()))?;
    for entry in read {
        let entry = entry.map_err(|e| SkillError::Message(e.to_string()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| SkillError::Message(e.to_string()))?;
        }
    }
    Ok(())
}

fn map_settings(err: SettingsError) -> SkillError {
    SkillError::Message(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn write_skill(dir: &Path, name: &str, description: &str) {
        let root = dir.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(SKILL_MD),
            format!("---\nname: {name}\ndescription: {description}\n---\n# Body\n"),
        )
        .unwrap();
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        with_temp_home(|| {
            assert!(scan_user_skills().unwrap().is_empty());
        });
    }

    #[test]
    fn scan_and_load_round_trip() {
        with_temp_home(|| {
            let dir = ensure_user_skills_dir().unwrap();
            write_skill(&dir, "my-skill", "User skill for filling REST method docs.");
            fs::write(dir.join("my-skill").join("notes.md"), "extra").unwrap();
            let metas = valid_user_metas().unwrap();
            assert_eq!(metas.len(), 1);
            assert_eq!(metas[0].name, "my-skill");
            let files = companion_files("my-skill").unwrap();
            assert_eq!(files, vec!["notes.md"]);
            let text = read_companion("my-skill", "notes.md").unwrap();
            assert_eq!(text, "extra");
        });
    }

    #[test]
    fn read_companion_rejects_path_escape() {
        with_temp_home(|| {
            let dir = ensure_user_skills_dir().unwrap();
            write_skill(&dir, "my-skill", "User skill for filling REST method docs.");
            let err = read_companion("my-skill", "../secrets.txt").unwrap_err();
            assert!(matches!(err, SkillError::PathEscape(_)));
        });
    }

    #[test]
    fn import_requires_skill_md() {
        with_temp_home(|| {
            let tmp = std::env::temp_dir().join(format!(
                "atlas-skill-import-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&tmp).unwrap();
            let empty = tmp.join("empty-skill");
            fs::create_dir_all(&empty).unwrap();
            let err = import_skill_dir(&empty).unwrap_err();
            fs::remove_dir_all(&tmp).ok();
            assert!(err.to_string().contains("SKILL.md"));
        });
    }

    #[test]
    fn import_then_remove() {
        with_temp_home(|| {
            let tmp = std::env::temp_dir().join(format!(
                "atlas-skill-import-ok-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            write_skill(&tmp, "imported-skill", "Imported user skill for OpenAPI layout.");
            let meta = import_skill_dir(&tmp.join("imported-skill")).unwrap();
            assert_eq!(meta.name, "imported-skill");
            assert_eq!(meta.source, SkillSource::User);
            remove_user_skill("imported-skill").unwrap();
            assert!(valid_user_metas().unwrap().is_empty());
            fs::remove_dir_all(&tmp).ok();
        });
    }
}
