//! Profiles: separate people (or work/personal) on one Mac. Each profile has
//! its own chats, memories, settings and database key; downloaded models are
//! shared. The default profile lives directly in the app data folder (so
//! nothing moves for existing users); others live in `profiles/<id>/`.
//! Switching profiles restarts BYTE.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const DEFAULT_ID: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profiles {
    pub active: String,
    pub profiles: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Profiles { active: DEFAULT_ID.into(), profiles: vec![Profile { id: DEFAULT_ID.into(), name: "Me".into(), created_at: 0 }] }
    }
}

fn file(root: &Path) -> PathBuf {
    root.join("profiles.json")
}

impl Profiles {
    pub fn load(root: &Path) -> Profiles {
        let mut p: Profiles = std::fs::read_to_string(file(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if !p.profiles.iter().any(|x| x.id == DEFAULT_ID) {
            p.profiles.insert(0, Profiles::default().profiles.remove(0));
        }
        if !p.profiles.iter().any(|x| x.id == p.active) {
            p.active = DEFAULT_ID.into();
        }
        p
    }

    pub fn save(&self, root: &Path) -> AppResult<()> {
        let tmp = file(root).with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, file(root))?;
        Ok(())
    }

    /// Where a profile keeps its data.
    pub fn dir(root: &Path, id: &str) -> PathBuf {
        if id == DEFAULT_ID {
            root.to_path_buf()
        } else {
            root.join("profiles").join(id)
        }
    }

    pub fn create(&mut self, root: &Path, name: &str) -> AppResult<Profile> {
        let name = clean_name(name)?;
        let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let id = format!("{}-{}", slug.trim_matches('-').chars().take(20).collect::<String>(), &uuid::Uuid::new_v4().simple().to_string()[..6]);
        let p = Profile { id, name, created_at: chrono::Utc::now().timestamp_millis() };
        std::fs::create_dir_all(Self::dir(root, &p.id))?;
        self.profiles.push(p.clone());
        self.save(root)?;
        Ok(p)
    }

    pub fn rename(&mut self, root: &Path, id: &str, name: &str) -> AppResult<()> {
        let name = clean_name(name)?;
        let p = self.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| AppError::msg("No such profile."))?;
        p.name = name;
        self.save(root)
    }

    /// Deletes a profile and everything in it (not the default or active one).
    pub fn delete(&mut self, root: &Path, id: &str) -> AppResult<()> {
        if id == DEFAULT_ID || id == self.active {
            return Err(AppError::msg("Switch to another profile before deleting this one."));
        }
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.profiles.len() == before {
            return Err(AppError::msg("No such profile."));
        }
        let dir = Self::dir(root, id);
        // Only ever delete inside `profiles/`.
        if dir.starts_with(root.join("profiles")) && dir != root.join("profiles") {
            let _ = std::fs::remove_dir_all(dir);
        }
        self.save(root)
    }

    pub fn set_active(&mut self, root: &Path, id: &str) -> AppResult<()> {
        if !self.profiles.iter().any(|p| p.id == id) {
            return Err(AppError::msg("No such profile."));
        }
        self.active = id.into();
        self.save(root)
    }
}

fn clean_name(name: &str) -> AppResult<String> {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    if n.is_empty() {
        return Err(AppError::msg("Give the profile a name."));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_switch_rename_delete() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let mut p = Profiles::load(r);
        assert_eq!(p.active, DEFAULT_ID);
        assert_eq!(Profiles::dir(r, DEFAULT_ID), r);
        let work = p.create(r, "Work / Logan").unwrap();
        assert!(work.id.starts_with("work---logan-") || work.id.starts_with("work"));
        assert!(Profiles::dir(r, &work.id).starts_with(r.join("profiles")));
        assert!(Profiles::dir(r, &work.id).is_dir());
        p.set_active(r, &work.id).unwrap();
        assert!(p.delete(r, &work.id).is_err(), "can't delete the active profile");
        assert!(p.delete(r, DEFAULT_ID).is_err());
        p.rename(r, &work.id, "Work").unwrap();
        let reloaded = Profiles::load(r);
        assert_eq!(reloaded.active, work.id);
        assert_eq!(reloaded.profiles[1].name, "Work");
        p.set_active(r, DEFAULT_ID).unwrap();
        p.delete(r, &work.id).unwrap();
        assert!(!Profiles::dir(r, &work.id).exists());
        assert!(p.create(r, "   ").is_err());
    }

    #[test]
    fn a_missing_or_broken_file_means_the_default_profile() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("profiles.json"), "{not json").unwrap();
        assert_eq!(Profiles::load(root.path()), Profiles::default());
        std::fs::write(root.path().join("profiles.json"), r#"{"active":"gone","profiles":[]}"#).unwrap();
        let p = Profiles::load(root.path());
        assert_eq!(p.active, DEFAULT_ID);
        assert_eq!(p.profiles.len(), 1);
    }
}
