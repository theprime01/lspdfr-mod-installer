//! Copies a plan into the game folder, remembering every file so the mod
//! can be removed again, and backing up any game file it replaces.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze::{Action, Kind, Plan};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRec {
    /// Relative to the game folder.
    pub path: PathBuf,
    /// The file that was there before, if any.
    pub backup: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mod {
    pub id: u64,
    pub name: String,
    pub source: String,
    pub installed: u64,
    pub kinds: Vec<Kind>,
    pub files: Vec<FileRec>,
    pub game: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Db {
    pub next_id: u64,
    pub mods: Vec<Mod>,
    #[serde(skip)]
    pub path: PathBuf,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("LSPDFR Mod Installer")
}

impl Db {
    pub fn load(dir: &Path) -> Db {
        let path = dir.join("installed.json");
        let mut db: Db = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        db.path = path;
        db
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    fn backups(&self) -> PathBuf {
        self.path.parent().unwrap_or(Path::new(".")).join("backups")
    }

    pub fn find(&self, game: &Path, name: &str) -> Option<u64> {
        self.mods.iter().find(|m| same(&m.game, game) && m.name.eq_ignore_ascii_case(name)).map(|m| m.id)
    }

    /// Mods (other than `id`) that installed this file, in install order.
    fn owners(&self, game: &Path, rel: &Path, id: u64) -> Vec<u64> {
        self.mods.iter().filter(|m| m.id != id && same(&m.game, game) && m.files.iter().any(|f| same(&f.path, rel))).map(|m| m.id).collect()
    }

    /// Installs the chosen parts of `plan`. A mod of the same name is
    /// replaced (an update). Returns what was installed and any notes.
    pub fn install(&mut self, game: &Path, plan: &Plan, source: &str) -> Result<(Mod, Vec<String>), String> {
        let mut notes = Vec::new();
        if let Some(old) = self.find(game, &plan.name) {
            self.uninstall(old)?;
            notes.push("Replaced the version installed before.".to_string());
        }
        self.add(game, &plan.name, source, plan.parts().map(|p| (p.kind, p.actions.as_slice())), notes)
    }

    /// Adds more parts to a mod (or creates it).
    pub fn add<'a>(&mut self, game: &Path, name: &str, source: &str, parts: impl Iterator<Item = (Kind, &'a [Action])>, mut notes: Vec<String>) -> Result<(Mod, Vec<String>), String> {
        let existing = self.find(game, name);
        let id = existing.unwrap_or_else(|| {
            self.next_id += 1;
            self.next_id
        });
        let backups = self.backups().join(id.to_string());
        let mut files: Vec<FileRec> = Vec::new();
        let mut kinds = Vec::new();
        let result = (|| -> Result<(), String> {
            for (kind, actions) in parts {
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
                for action in actions {
                    let rel = action.to();
                    if rel.as_os_str().is_empty() {
                        continue;
                    }
                    if let Some(why) = protected(rel) {
                        notes.push(format!("Left out {}: {why}", rel.display()));
                        continue;
                    }
                    let dest = game.join(rel);
                    if dest.is_dir() {
                        notes.push(format!("Left out {}: a folder is in the way", rel.display()));
                        continue;
                    }
                    let backup = if dest.is_file() && !files.iter().any(|f| same(&f.path, rel)) {
                        let b = backups.join(rel);
                        fs::create_dir_all(b.parent().unwrap()).map_err(|e| e.to_string())?;
                        fs::copy(&dest, &b).map_err(|e| format!("backing up {}: {e}", rel.display()))?;
                        Some(b)
                    } else {
                        None
                    };
                    if let Some(parent) = dest.parent() {
                        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    let written = match action {
                        Action::Copy { from, .. } => fs::copy(from, &dest).map(|_| ()),
                        Action::Write { contents, .. } => fs::write(&dest, contents),
                    };
                    if !files.iter().any(|f| same(&f.path, rel)) {
                        files.push(FileRec { path: rel.to_path_buf(), backup });
                    }
                    written.map_err(|e| format!("{}: {e}", rel.display()))?;
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            // Put back everything this attempt touched.
            for f in files.iter().rev() {
                restore(game, f);
            }
            return Err(e);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let record = match self.mods.iter_mut().find(|m| m.id == id) {
            Some(m) => {
                for f in files {
                    if !m.files.iter().any(|g| same(&g.path, &f.path)) {
                        m.files.push(f);
                    }
                }
                for k in kinds {
                    if !m.kinds.contains(&k) {
                        m.kinds.push(k);
                    }
                }
                m.installed = now;
                m.clone()
            }
            None => {
                let m = Mod { id, name: name.to_string(), source: source.to_string(), installed: now, kinds, files, game: game.to_path_buf() };
                self.mods.push(m.clone());
                m
            }
        };
        self.save()?;
        Ok((record, notes))
    }

    /// Removes a mod: deletes its files and puts back what they replaced.
    /// Files a later mod also installed stay, and that mod inherits the
    /// backup of the original.
    pub fn uninstall(&mut self, id: u64) -> Result<(), String> {
        let Some(index) = self.mods.iter().position(|m| m.id == id) else { return Ok(()) };
        let m = self.mods[index].clone();
        let order: Vec<u64> = self.mods.iter().map(|m| m.id).collect();
        let position = |id: u64| order.iter().position(|&o| o == id).unwrap_or(0);
        let backups = self.backups();
        for f in &m.files {
            let later = self.owners(&m.game, &f.path, id).into_iter().find(|&o| position(o) > index);
            match later {
                Some(other) => {
                    let other = self.mods.iter_mut().find(|o| o.id == other).unwrap();
                    let rec = other.files.iter_mut().find(|g| same(&g.path, &f.path)).unwrap();
                    if let Some(old) = rec.backup.take() {
                        let _ = fs::remove_file(old);
                    }
                    // Move the original into the other mod's backups.
                    rec.backup = f.backup.as_ref().and_then(|b| {
                        let moved = backups.join(other.id.to_string()).join(&f.path);
                        fs::create_dir_all(moved.parent()?).ok()?;
                        fs::rename(b, &moved).or_else(|_| fs::copy(b, &moved).map(|_| ())).ok()?;
                        Some(moved)
                    });
                }
                None => restore(&m.game, f),
            }
        }
        for f in &m.files {
            prune(&m.game, &f.path);
        }
        let _ = fs::remove_dir_all(self.backups().join(id.to_string()));
        self.mods.retain(|x| x.id != id);
        self.save()
    }
}

/// Puts back the original of a file, or deletes it if there was none.
fn restore(game: &Path, f: &FileRec) {
    let dest = game.join(&f.path);
    match &f.backup {
        Some(b) if b.is_file() => {
            let _ = fs::copy(b, &dest);
        }
        _ => {
            let _ = fs::remove_file(&dest);
        }
    }
}

const KEEP: &[&str] = &["plugins", r"plugins\lspdfr", "scripts", "lml", "els", r"els\pack_default", "lspdfr"];

/// Removes folders left empty, up to the game folder.
fn prune(game: &Path, rel: &Path) {
    let mut dir = rel.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d.as_os_str().is_empty() || KEEP.contains(&d.to_string_lossy().to_lowercase().as_str()) {
            break;
        }
        if fs::remove_dir(game.join(&d)).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

/// Why a destination must not be written, if it mustn't.
pub fn protected(rel: &Path) -> Option<&'static str> {
    let comps: Vec<String> = rel.components().map(|c| match c {
        Component::Normal(s) => s.to_string_lossy().to_lowercase(),
        _ => "..".to_string(),
    }).collect();
    if comps.is_empty() || comps.iter().any(|c| c == "..") || rel.is_absolute() {
        return Some("outside the game folder");
    }
    if matches!(comps[0].as_str(), "update" | "x64" | "battleye" | "redistributables") {
        return Some("that's one of the game's own folders");
    }
    if comps.len() == 1 {
        let name = comps[0].as_str();
        if name.ends_with(".rpf") || matches!(name, "gta5.exe" | "gta5_be.exe" | "playgtav.exe" | "gtavlauncher.exe" | "gtavlanguageselect.exe" | "steam_api64.dll") {
            return Some("that's one of the game's own files");
        }
    }
    None
}

fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().replace('/', "\\").eq_ignore_ascii_case(&b.to_string_lossy().replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::{Part, Set};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lspdfr-install-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn plan(name: &str, files: &[(&str, &str)]) -> Plan {
        let actions = files.iter().map(|(to, text)| Action::Write { to: PathBuf::from(to), contents: text.to_string() }).collect();
        Plan {
            name: name.to_string(),
            exclusive: false,
            sets: vec![Set { label: name.into(), selected: true, skipped: vec![], parts: vec![Part { kind: Kind::LspdfrPlugin, label: name.into(), actions }] }],
        }
    }

    #[test]
    fn uninstall_restores_what_was_there_and_keeps_shared_files() {
        let root = tmp("shared");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("RAGENativeUI.dll"), "original").unwrap();
        let mut db = Db::load(&root.join("data"));

        db.install(&game, &plan("A", &[("RAGENativeUI.dll", "a"), (r"plugins\LSPDFR\A.dll", "a")]), "a.zip").unwrap();
        db.install(&game, &plan("B", &[("RAGENativeUI.dll", "b")]), "b.zip").unwrap();
        let read = |p: &str| fs::read_to_string(game.join(p)).ok();
        assert_eq!(read("RAGENativeUI.dll").as_deref(), Some("b"));

        // A goes: B's copy stays, and B now owns the original.
        let a = db.find(&game, "A").unwrap();
        db.uninstall(a).unwrap();
        assert_eq!(read("RAGENativeUI.dll").as_deref(), Some("b"));
        assert_eq!(read(r"plugins\LSPDFR\A.dll"), None);
        assert!(game.join("plugins").join("LSPDFR").is_dir(), "plugin folders are kept");

        let b = db.find(&game, "B").unwrap();
        db.uninstall(b).unwrap();
        assert_eq!(read("RAGENativeUI.dll").as_deref(), Some("original"));
        assert!(Db::load(&root.join("data")).mods.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reinstalling_replaces_the_old_version_and_lml_folders_are_removed() {
        let root = tmp("update");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        let mut db = Db::load(&root.join("data"));
        db.install(&game, &plan("Car", &[(r"lml\Car\stream\car.yft", "1"), (r"lml\Car\install.xml", "x")]), "car.zip").unwrap();
        db.install(&game, &plan("Car", &[(r"lml\Car\stream\car2.yft", "2"), (r"lml\Car\install.xml", "y")]), "car.zip").unwrap();
        assert!(!game.join(r"lml\Car\stream\car.yft").exists());
        assert_eq!(db.mods.len(), 1);
        db.uninstall(db.mods[0].id).unwrap();
        assert!(!game.join(r"lml\Car").exists());
        assert!(game.join("lml").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn never_touches_the_games_own_files() {
        assert!(protected(Path::new("x64a.rpf")).is_some());
        assert!(protected(Path::new(r"update\update.rpf")).is_some());
        assert!(protected(Path::new("GTA5.exe")).is_some());
        assert!(protected(Path::new(r"..\evil.dll")).is_some());
        assert!(protected(Path::new(r"plugins\LSPDFR\x.dll")).is_none());
        assert!(protected(Path::new(r"lml\car\dlc.rpf")).is_none());
    }
}
