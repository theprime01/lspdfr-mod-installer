//! Copies a plan into the game folder, remembering every file so the mod
//! can be removed again, and backing up any game file it replaces.
//!
//! Mods can also be switched off without removing them: their files move
//! to a folder the game never reads, and come back when switched on.
//! Profiles (e.g. American and British) are sets of mods switched on
//! together, and the clean game switches every mod off.
//!
//! For a file several mods installed, the newest switched-on mod's copy is
//! in the game folder, and each switched-on mod's backup holds the copy
//! that was there before it. A switched-off mod's own copies are parked
//! and it has no backups.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze::{Action, Kind, Plan};
use crate::eup;

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
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The profiles this mod is in. None: every profile.
    #[serde(default)]
    pub profiles: Option<Vec<String>>,
    /// Spawn names of the cars in it.
    #[serde(default)]
    pub models: Vec<String>,
}

fn yes() -> bool {
    true
}

impl Mod {
    /// Whether this mod is on in a profile (None: "All mods").
    pub fn in_profile(&self, profile: Option<&str>) -> bool {
        match (profile, &self.profiles) {
            (None, _) | (_, None) => true,
            (Some(p), Some(list)) => list.iter().any(|x| x.eq_ignore_ascii_case(p)),
        }
    }

    pub fn is_car(&self) -> bool {
        self.kinds.iter().any(|k| matches!(k, Kind::AddonVehicle | Kind::ReplaceVehicle | Kind::DlcPack))
    }
}

/// Profiles and switches for one game folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameState {
    pub game: PathBuf,
    pub profiles: Vec<String>,
    /// The profile that's on. None: every mod is on.
    pub active: Option<String>,
    /// While the clean game is on: the mods that were on before.
    pub clean: Option<Vec<u64>>,
    /// Plugins whose LoadPlugin line in startup.rphs this app added.
    #[serde(default)]
    pub startup: Vec<String>,
}

/// Another mod (or a file already there) that an install would replace.
#[derive(Clone, Debug, PartialEq)]
pub struct Clash {
    /// Empty for files that weren't installed by this app.
    pub name: String,
    pub files: usize,
    /// Only shared libraries like RAGENativeUI.dll, which is normal.
    pub libraries_only: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Db {
    pub next_id: u64,
    pub mods: Vec<Mod>,
    #[serde(default)]
    pub games: Vec<GameState>,
    #[serde(skip)]
    pub path: PathBuf,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("LSPDFR Mod Installer")
}

/// The folder in the game folder that holds switched-off mods. The game,
/// RAGE Plugin Hook, Script Hook V and LML never look inside it.
pub const PARKED: &str = "LSPDFR Mod Installer";

fn parked(game: &Path, id: u64) -> PathBuf {
    game.join(PARKED).join("switched off").join(id.to_string())
}

impl Db {
    pub fn load(dir: &Path) -> Db {
        let path = dir.join("installed.json");
        let mut db: Db = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        db.path = path;
        // Records from before spawn names were kept.
        for m in db.mods.iter_mut().filter(|m| m.is_car() && m.models.is_empty()) {
            m.models = spawn_names(m);
        }
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

    pub fn state(&self, game: &Path) -> GameState {
        self.games.iter().find(|s| same(&s.game, game)).cloned().unwrap_or_else(|| GameState { game: game.to_path_buf(), ..Default::default() })
    }

    fn state_mut(&mut self, game: &Path) -> &mut GameState {
        match self.games.iter().position(|s| same(&s.game, game)) {
            Some(i) => &mut self.games[i],
            None => {
                self.games.push(GameState { game: game.to_path_buf(), ..Default::default() });
                self.games.last_mut().unwrap()
            }
        }
    }

    /// The next switched-on mod after `index` that also installed `rel`, as
    /// (mod index, file index).
    fn above(&self, index: usize, rel: &Path) -> Option<(usize, usize)> {
        let game = &self.mods[index].game;
        self.mods.iter().enumerate().skip(index + 1).filter(|(_, m)| m.enabled && same(&m.game, game)).find_map(|(u, m)| m.files.iter().position(|f| same(&f.path, rel)).map(|k| (u, k)))
    }

    /// Installs the chosen parts of `plan`. A mod of the same name is
    /// replaced (an update) and keeps its profiles. A new mod goes into the
    /// profile that's on, unless it's `core` (something every profile
    /// needs, like LSPDFR itself). Returns what was installed and any notes.
    pub fn install(&mut self, game: &Path, plan: &Plan, source: &str, core: bool) -> Result<(Mod, Vec<String>), String> {
        let mut notes = Vec::new();
        let state = self.state(game);
        let mut profiles = if core { None } else { state.active.map(|p| vec![p]) };
        if let Some(old) = self.find(game, &plan.name) {
            profiles = self.mods.iter().find(|m| m.id == old).and_then(|m| m.profiles.clone());
            self.uninstall(old)?;
            notes.push("Replaced the version installed before.".to_string());
        }
        self.add(game, &plan.name, source, plan.parts().map(|p| (p.kind, p.actions.as_slice())), notes, profiles)
    }

    /// Adds more parts to a mod (or creates it, in `profiles`).
    pub fn add<'a>(&mut self, game: &Path, name: &str, source: &str, parts: impl Iterator<Item = (Kind, &'a [Action])>, mut notes: Vec<String>, profiles: Option<Vec<String>>) -> Result<(Mod, Vec<String>), String> {
        let existing = self.find(game, name);
        if existing.is_some_and(|id| self.mods.iter().any(|m| m.id == id && !m.enabled)) {
            return Err(format!("{name} is switched off. Switch it on first."));
        }
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
                    if let Action::Copy { from, .. } = action {
                        if let Some((have, new)) = newer_already(&dest, from) {
                            notes.push(format!("Kept {} {have}: it's newer than the {new} this mod comes with.", rel.display()));
                            continue;
                        }
                    }
                    let backup =if dest.is_file() && !files.iter().any(|f| same(&f.path, rel)) {
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
        let index = match self.mods.iter().position(|m| m.id == id) {
            Some(i) => {
                let m = &mut self.mods[i];
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
                i
            }
            None => {
                let m = Mod { id, name: name.to_string(), source: source.to_string(), installed: now, kinds, files, game: game.to_path_buf(), enabled: true, profiles, models: Vec::new() };
                self.mods.push(m);
                self.mods.len() - 1
            }
        };
        if self.mods[index].is_car() {
            self.mods[index].models = spawn_names(&self.mods[index]);
        }
        match self.refresh(game) {
            Ok(added) => notes.extend(added),
            Err(e) => notes.push(e),
        }
        self.save()?;
        Ok((self.mods[index].clone(), notes))
    }

    /// Removes a mod: deletes its files and puts back what they replaced.
    /// Files a later mod also installed stay, and that mod inherits the
    /// backup of the original.
    pub fn uninstall(&mut self, id: u64) -> Result<(), String> {
        let Some(index) = self.mods.iter().position(|m| m.id == id) else { return Ok(()) };
        let m = self.mods[index].clone();
        let backups = self.backups();
        if m.enabled {
            for f in &m.files {
                match self.above(index, &f.path) {
                    Some((u, k)) => {
                        let other = &mut self.mods[u];
                        let other_id = other.id;
                        let rec = &mut other.files[k];
                        if let Some(old) = rec.backup.take() {
                            let _ = fs::remove_file(old);
                        }
                        // The original goes into the other mod's backups.
                        rec.backup = f.backup.as_ref().and_then(|b| {
                            let moved = backups.join(other_id.to_string()).join(&f.path);
                            mv(b, &moved).ok()?;
                            Some(moved)
                        });
                    }
                    None => restore(&m.game, f),
                }
            }
            for f in &m.files {
                prune(&m.game, &f.path);
            }
        } else {
            let _ = fs::remove_dir_all(parked(&m.game, id));
            tidy_parked(&m.game);
        }
        let _ = fs::remove_dir_all(backups.join(id.to_string()));
        self.mods.retain(|x| x.id != id);
        // The mod is gone either way; a stale LoadPlugin line only prints
        // an error in the RPH console.
        let _ = self.refresh(&m.game);
        self.save()
    }

    /// Switches a mod on or off. Returns files that couldn't be moved.
    pub fn set_enabled(&mut self, id: u64, on: bool) -> Result<Vec<String>, String> {
        let Some(index) = self.mods.iter().position(|m| m.id == id) else { return Ok(Vec::new()) };
        let mut problems = match (self.mods[index].enabled, on) {
            (true, false) => self.switch_off(index),
            (false, true) => self.switch_on(index),
            _ => Vec::new(),
        };
        let game = self.mods[index].game.clone();
        problems.extend(self.refresh(&game).err());
        self.save()?;
        Ok(problems)
    }

    fn switch_off(&mut self, index: usize) -> Vec<String> {
        let (id, game) = (self.mods[index].id, self.mods[index].game.clone());
        let park = parked(&game, id);
        let backups = self.backups();
        let mut problems = Vec::new();
        // Clear the flag first so `above` only sees the other mods.
        self.mods[index].enabled = false;
        for j in 0..self.mods[index].files.len() {
            let rel = self.mods[index].files[j].path.clone();
            let mine = self.mods[index].files[j].backup.take();
            let dest = game.join(&rel);
            let result = (|| -> io::Result<()> {
                match self.above(index, &rel) {
                    // This mod's copy is in the game: park it, and put back
                    // what was there before.
                    None => {
                        if dest.is_file() {
                            mv(&dest, &park.join(&rel))?;
                        }
                        if let Some(b) = mine.as_ref().filter(|b| b.is_file()) {
                            mv(b, &dest)?;
                        }
                    }
                    // A later mod's copy is in the game, and its backup is
                    // this mod's copy: park that, and the later mod now sits
                    // on what was under this one.
                    Some((u, k)) => {
                        if let Some(b) = self.mods[u].files[k].backup.take().filter(|b| b.is_file()) {
                            mv(&b, &park.join(&rel))?;
                        }
                        let to = backups.join(self.mods[u].id.to_string()).join(&rel);
                        self.mods[u].files[k].backup = match mine.as_ref().filter(|b| b.is_file()) {
                            Some(b) => {
                                mv(b, &to)?;
                                Some(to)
                            }
                            None => None,
                        };
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                problems.push(format!("{}: {e}", rel.display()));
                self.mods[index].files[j].backup = mine.filter(|b| b.is_file());
            }
        }
        for f in &self.mods[index].files {
            prune(&game, &f.path);
        }
        if problems.is_empty() {
            let _ = fs::remove_dir_all(backups.join(id.to_string()));
        }
        problems
    }

    fn switch_on(&mut self, index: usize) -> Vec<String> {
        let (id, game) = (self.mods[index].id, self.mods[index].game.clone());
        let park = parked(&game, id);
        let backups = self.backups();
        let mut problems = Vec::new();
        for j in 0..self.mods[index].files.len() {
            let rel = self.mods[index].files[j].path.clone();
            let dest = game.join(&rel);
            let mine = backups.join(id.to_string()).join(&rel);
            let result = (|| -> io::Result<()> {
                match self.above(index, &rel) {
                    // Back on top: what's in the game now becomes the backup.
                    None if park.join(&rel).is_file() => {
                        if dest.is_file() {
                            mv(&dest, &mine)?;
                            self.mods[index].files[j].backup = Some(mine.clone());
                        }
                        mv(&park.join(&rel), &dest)?;
                    }
                    // Its copy went missing while it was off.
                    None => {}
                    // Under a later mod: take over its backup, and this
                    // mod's copy becomes that mod's backup.
                    Some((u, k)) => {
                        if let Some(b) = self.mods[u].files[k].backup.take().filter(|b| b.is_file()) {
                            mv(&b, &mine)?;
                            self.mods[index].files[j].backup = Some(mine.clone());
                        }
                        if park.join(&rel).is_file() {
                            let to = backups.join(self.mods[u].id.to_string()).join(&rel);
                            mv(&park.join(&rel), &to)?;
                            self.mods[u].files[k].backup = Some(to);
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                problems.push(format!("{}: {e}", rel.display()));
            }
        }
        self.mods[index].enabled = true;
        if problems.is_empty() {
            let _ = fs::remove_dir_all(&park);
            tidy_parked(&game);
        }
        problems
    }

    /// Switches mods so exactly those `wanted` are on.
    fn sync(&mut self, game: &Path, wanted: impl Fn(&Mod) -> bool) -> Vec<String> {
        let mut problems = Vec::new();
        for i in (0..self.mods.len()).rev() {
            if same(&self.mods[i].game, game) && self.mods[i].enabled && !wanted(&self.mods[i]) {
                problems.extend(self.switch_off(i));
            }
        }
        for i in 0..self.mods.len() {
            if same(&self.mods[i].game, game) && !self.mods[i].enabled && wanted(&self.mods[i]) {
                problems.extend(self.switch_on(i));
            }
        }
        problems
    }

    /// Switches to a profile (None: every mod on). Ends the clean game.
    pub fn use_profile(&mut self, game: &Path, profile: Option<String>) -> Result<Vec<String>, String> {
        let state = self.state_mut(game);
        state.clean = None;
        state.active = profile.clone();
        let mut problems = self.sync(game, |m| m.in_profile(profile.as_deref()));
        problems.extend(self.refresh(game).err());
        self.save()?;
        Ok(problems)
    }

    /// The clean game: every mod off. Switching it off again puts back the
    /// mods that were on.
    pub fn set_clean(&mut self, game: &Path, on: bool) -> Result<Vec<String>, String> {
        let state = self.state(game);
        let mut problems = match (state.clean, on) {
            (None, true) => {
                let was_on: Vec<u64> = self.mods.iter().filter(|m| same(&m.game, game) && m.enabled).map(|m| m.id).collect();
                self.state_mut(game).clean = Some(was_on);
                self.sync(game, |_| false)
            }
            (Some(was_on), false) => {
                self.state_mut(game).clean = None;
                let mut problems = Vec::new();
                for i in 0..self.mods.len() {
                    if was_on.contains(&self.mods[i].id) && !self.mods[i].enabled {
                        problems.extend(self.switch_on(i));
                    }
                }
                problems
            }
            _ => Vec::new(),
        };
        problems.extend(self.refresh(game).err());
        self.save()?;
        Ok(problems)
    }

    /// Makes startup.rphs load the RAGE Plugin Hook plugins of the mods
    /// that are on. Only lines this app added are ever taken out. Returns
    /// the plugins it added.
    fn refresh_startup(&mut self, game: &Path) -> Result<Vec<String>, String> {
        let mut wanted: Vec<String> = Vec::new();
        for m in self.mods.iter().filter(|m| m.enabled && same(&m.game, game)) {
            for f in m.files.iter().filter(|f| is_rph_plugin(game, &f.path)) {
                let name = f.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                if !wanted.iter().any(|w| w.eq_ignore_ascii_case(&name)) {
                    wanted.push(name);
                }
            }
        }
        let owned = self.state(game).startup;
        let path = game.join(STARTUP);
        let text = fs::read_to_string(&path).unwrap_or_default();
        let (new_text, new_owned) = startup_lines(&text, &wanted, &owned);
        if new_text != text {
            fs::write(&path, &new_text).map_err(|e| format!("Couldn't update {STARTUP}: {e}"))?;
        }
        let added = new_owned.iter().filter(|p| !owned.iter().any(|o| o.eq_ignore_ascii_case(p))).cloned().collect();
        if new_owned != owned {
            self.state_mut(game).startup = new_owned;
        }
        Ok(added)
    }

    /// The shared files that follow which mods are on: startup.rphs and EUP
    /// Menu's preset outfits. Returns notes about what was added.
    fn refresh(&mut self, game: &Path) -> Result<Vec<String>, String> {
        let mut notes: Vec<String> = self.refresh_startup(game)?.iter().map(|p| format!("RAGE Plugin Hook will load {p} when the game starts (added to {STARTUP}).")).collect();
        notes.extend(self.refresh_presets(game)?.iter().map(|a| format!("EUP Menu has a {a} section now (press its key in game, then EUP).")));
        Ok(notes)
    }

    /// Puts the outfits of the EUP add-on packs that are on into
    /// presetoutfits.ini, each in its own marked block, and takes out the
    /// blocks of packs that aren't. Returns the packs it added.
    fn refresh_presets(&mut self, game: &Path) -> Result<Vec<String>, String> {
        let packs_dir = Path::new(eup::PACKS);
        let mut packs: Vec<(String, String)> = Vec::new();
        for m in self.mods.iter().filter(|m| m.enabled && same(&m.game, game)) {
            for f in m.files.iter().filter(|f| f.path.parent().is_some_and(|p| same(p, packs_dir))) {
                if let Ok(text) = fs::read_to_string(game.join(&f.path)) {
                    packs.push((f.path.file_stem().unwrap_or_default().to_string_lossy().into_owned(), text));
                }
            }
        }
        let path = game.join(eup::PRESETS);
        let Ok(text) = fs::read_to_string(&path) else {
            if packs.is_empty() {
                return Ok(Vec::new());
            }
            return Err(format!("EUP Menu's preset outfits ({}) aren't there yet, so this pack's outfits wait. Install EUP Law & Order, then switch this mod off and on.", eup::PRESETS));
        };
        let marker = |p: &str| format!("// >>> LSPDFR Mod Installer: {p}");
        if packs.is_empty() && !text.contains("// >>> LSPDFR Mod Installer: ") {
            return Ok(Vec::new());
        }
        let new_text = eup::presets_with(&text, &packs);
        if new_text != text {
            fs::write(&path, &new_text).map_err(|e| format!("Couldn't update {}: {e}", eup::PRESETS))?;
        }
        Ok(packs.into_iter().map(|(p, _)| p).filter(|p| !text.contains(&marker(p))).collect())
    }

    pub fn add_profile(&mut self, game: &Path, name: &str) -> Result<(), String> {
        let name = name.trim();
        let state = self.state_mut(game);
        if name.is_empty() || name.eq_ignore_ascii_case("All mods") || state.profiles.iter().any(|p| p.eq_ignore_ascii_case(name)) {
            return Ok(());
        }
        state.profiles.push(name.to_string());
        self.save()
    }

    /// Deletes a profile. Mods only in it stay installed and are on under
    /// "All mods".
    pub fn remove_profile(&mut self, game: &Path, name: &str) -> Result<Vec<String>, String> {
        let state = self.state_mut(game);
        state.profiles.retain(|p| !p.eq_ignore_ascii_case(name));
        let was_active = state.active.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name));
        for m in self.mods.iter_mut().filter(|m| same(&m.game, game)) {
            if let Some(list) = &mut m.profiles {
                list.retain(|p| !p.eq_ignore_ascii_case(name));
            }
        }
        if was_active {
            return self.use_profile(game, None);
        }
        self.save()?;
        Ok(Vec::new())
    }

    /// Puts a mod in these profiles (None: every profile), and switches it
    /// on or off to match the profile that's on.
    pub fn set_profiles(&mut self, id: u64, profiles: Option<Vec<String>>) -> Result<Vec<String>, String> {
        let Some(m) = self.mods.iter_mut().find(|m| m.id == id) else { return Ok(Vec::new()) };
        m.profiles = profiles;
        let (game, member) = (m.game.clone(), m.clone());
        let state = self.state(&game);
        if state.clean.is_none() && state.active.is_some() {
            return self.set_enabled(id, member.in_profile(state.active.as_deref()));
        }
        self.save()?;
        Ok(Vec::new())
    }

    /// Mods (and files already in the game folder) this plan would replace.
    pub fn clashes(&self, game: &Path, plan: &Plan) -> Vec<Clash> {
        let mut out: Vec<Clash> = Vec::new();
        for part in plan.parts() {
            for action in &part.actions {
                let rel = action.to();
                if rel.as_os_str().is_empty() || protected(rel).is_some() || !game.join(rel).is_file() {
                    continue;
                }
                let owner = self.mods.iter().rev().find(|m| m.enabled && same(&m.game, game) && m.files.iter().any(|f| same(&f.path, rel)));
                let name = owner.map(|m| m.name.clone()).unwrap_or_default();
                if name.eq_ignore_ascii_case(&plan.name) {
                    continue;
                }
                let library = part.kind == Kind::Library;
                match out.iter_mut().find(|c| c.name == name) {
                    Some(c) => {
                        c.files += 1;
                        c.libraries_only &= library;
                    }
                    None => out.push(Clash { name, files: 1, libraries_only: library }),
                }
            }
        }
        out
    }
}

/// Spawn names of a car mod, read from its files wherever they are now.
fn spawn_names(m: &Mod) -> Vec<String> {
    let base = if m.enabled { m.game.clone() } else { parked(&m.game, m.id) };
    let files: Vec<PathBuf> = m.files.iter().map(|f| base.join(&f.path)).collect();
    crate::lml::spawn_names(&files)
}

/// Moves a file, replacing what's at `to`, across drives if need be.
fn mv(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if to.is_file() {
        fs::remove_file(to)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// Removes the switched-off folders once nothing is parked.
fn tidy_parked(game: &Path) {
    let off = game.join(PARKED).join("switched off");
    let _ = fs::remove_dir(&off);
    let _ = fs::remove_dir(game.join(PARKED));
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
    if comps[0] == PARKED.to_lowercase() {
        return Some("that's where switched-off mods are kept");
    }
    if comps.len() == 1 {
        let name = comps[0].as_str();
        if name.ends_with(".rpf") || matches!(name, "gta5.exe" | "gta5_be.exe" | "playgtav.exe" | "gtavlauncher.exe" | "gtavlanguageselect.exe" | "steam_api64.dll") {
            return Some("that's one of the game's own files");
        }
    }
    None
}

/// For a .dll or .asi already in the game that's a newer version than the
/// one about to replace it: (version there, version in the mod).
fn newer_already(dest: &Path, from: &Path) -> Option<(String, String)> {
    let e = crate::extract::ext(dest);
    if !(e == "dll" || e == "asi") || !dest.is_file() {
        return None;
    }
    let (have, new) = (crate::dotnet::file_version(dest)?, crate::dotnet::file_version(from)?);
    let show = |v: [u16; 4]| format!("{}.{}.{}", v[0], v[1], v[2]);
    (have > new).then(|| (show(have), show(new)))
}

/// RAGE Plugin Hook runs the console commands in this file, in the game
/// folder, when the game starts.
pub const STARTUP: &str = "startup.rphs";

/// A plugin straight in `plugins\` (LSPDFR loads the ones in
/// `plugins\LSPDFR\` itself).
fn is_rph_plugin(game: &Path, rel: &Path) -> bool {
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    matches!(comps.as_slice(), [dir, file] if dir == "plugins" && file.ends_with(".dll")) && crate::dotnet::classify(&game.join(rel)) == crate::dotnet::Dll::RphPlugin
}

/// The plugin a `LoadPlugin` line loads.
fn loads(line: &str) -> Option<&str> {
    let line = line.trim();
    let head = line.get(..10)?;
    head.eq_ignore_ascii_case("loadplugin").then(|| line[10..].trim().trim_matches('"').trim())
}

/// startup.rphs with a LoadPlugin line for every `wanted` plugin, and
/// without the lines for `owned` plugins that aren't wanted any more.
/// Lines someone else wrote stay. Returns the text and the plugins whose
/// lines this app now owns.
fn startup_lines(text: &str, wanted: &[String], owned: &[String]) -> (String, Vec<String>) {
    let is = |list: &[String], p: &str| list.iter().any(|x| x.eq_ignore_ascii_case(p));
    let mut lines: Vec<&str> = text.lines().filter(|l| !loads(l).is_some_and(|p| is(owned, p) && !is(wanted, p))).collect();
    let removed = lines.len() != text.lines().count();
    let mut now_owned: Vec<String> = owned.iter().filter(|o| is(wanted, o)).cloned().collect();
    let mut added = Vec::new();
    for w in wanted {
        if !lines.iter().any(|l| loads(l).is_some_and(|p| p.eq_ignore_ascii_case(w))) {
            added.push(format!("LoadPlugin \"{w}\""));
            if !is(&now_owned, w) {
                now_owned.push(w.clone());
            }
        }
    }
    if !removed && added.is_empty() {
        return (text.to_string(), now_owned);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.extend(added.iter().map(String::as_str));
    let newline = if text.contains("\r\n") || text.is_empty() { "\r\n" } else { "\n" };
    let out = lines.iter().map(|l| format!("{l}{newline}")).collect();
    (out, now_owned)
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
            warnings: vec![],
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

        db.install(&game, &plan("A", &[("RAGENativeUI.dll", "a"), (r"plugins\LSPDFR\A.dll", "a")]), "a.zip", false).unwrap();
        db.install(&game, &plan("B", &[("RAGENativeUI.dll", "b")]), "b.zip", false).unwrap();
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
        db.install(&game, &plan("Car", &[(r"lml\Car\stream\car.yft", "1"), (r"lml\Car\install.xml", "x")]), "car.zip", false).unwrap();
        db.install(&game, &plan("Car", &[(r"lml\Car\stream\car2.yft", "2"), (r"lml\Car\install.xml", "y")]), "car.zip", false).unwrap();
        assert!(!game.join(r"lml\Car\stream\car.yft").exists());
        assert_eq!(db.mods.len(), 1);
        db.uninstall(db.mods[0].id).unwrap();
        assert!(!game.join(r"lml\Car").exists());
        assert!(game.join("lml").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn switching_off_and_on_in_any_order_keeps_every_copy() {
        let root = tmp("toggle");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("shared.dll"), "original").unwrap();
        let mut db = Db::load(&root.join("data"));
        let a = db.install(&game, &plan("A", &[("shared.dll", "a"), (r"plugins\LSPDFR\A.dll", "a")]), "a", false).unwrap().0.id;
        let b = db.install(&game, &plan("B", &[("shared.dll", "b")]), "b", false).unwrap().0.id;
        let c = db.install(&game, &plan("C", &[("shared.dll", "c")]), "c", false).unwrap().0.id;
        let read = |p: &str| fs::read_to_string(game.join(p)).ok();

        // B (in the middle) off: C stays in the game, and C's backup is now A's copy.
        assert!(db.set_enabled(b, false).unwrap().is_empty());
        assert_eq!(read("shared.dll").as_deref(), Some("c"));
        db.set_enabled(c, false).unwrap();
        assert_eq!(read("shared.dll").as_deref(), Some("a"));
        db.set_enabled(a, false).unwrap();
        assert_eq!(read("shared.dll").as_deref(), Some("original"));
        assert_eq!(read(r"plugins\LSPDFR\A.dll"), None);
        assert!(game.join(PARKED).is_dir());

        // Back on, out of order: the newest switched-on copy wins.
        db.set_enabled(b, true).unwrap();
        assert_eq!(read("shared.dll").as_deref(), Some("b"));
        db.set_enabled(a, true).unwrap();
        assert_eq!(read("shared.dll").as_deref(), Some("b"));
        assert_eq!(read(r"plugins\LSPDFR\A.dll").as_deref(), Some("a"));
        db.set_enabled(c, true).unwrap();
        assert_eq!(read("shared.dll").as_deref(), Some("c"));
        assert!(!game.join(PARKED).exists(), "nothing parked once all are on");

        // And everything still unwinds to the original.
        for id in [b, c, a] {
            db.uninstall(id).unwrap();
        }
        assert_eq!(read("shared.dll").as_deref(), Some("original"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn profiles_and_the_clean_game() {
        let root = tmp("profiles");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join(r"original.txt"), "x").unwrap();
        let mut db = Db::load(&root.join("data"));
        let read = |p: &str| fs::read_to_string(game.join(p)).ok();
        db.install(&game, &plan("LSPDFR", &[(r"plugins\LSPD First Response.dll", "core")]), "l", true).unwrap();
        db.add_profile(&game, "American").unwrap();
        db.add_profile(&game, "British").unwrap();
        db.use_profile(&game, Some("American".into())).unwrap();
        db.install(&game, &plan("US Scanner", &[(r"lspdfr\audio\scanner\a.wav", "us")]), "u", false).unwrap();
        db.use_profile(&game, Some("British".into())).unwrap();
        db.install(&game, &plan("UK Scanner", &[(r"lspdfr\audio\scanner\a.wav", "uk")]), "k", false).unwrap();
        assert_eq!(read(r"lspdfr\audio\scanner\a.wav").as_deref(), Some("uk"));

        db.use_profile(&game, Some("American".into())).unwrap();
        assert_eq!(read(r"lspdfr\audio\scanner\a.wav").as_deref(), Some("us"));
        assert_eq!(read(r"plugins\LSPD First Response.dll").as_deref(), Some("core"), "core mods are in every profile");

        db.set_clean(&game, true).unwrap();
        assert_eq!(read(r"lspdfr\audio\scanner\a.wav"), None);
        assert_eq!(read(r"plugins\LSPD First Response.dll"), None);
        assert_eq!(read("original.txt").as_deref(), Some("x"));
        db.set_clean(&game, false).unwrap();
        assert_eq!(read(r"lspdfr\audio\scanner\a.wav").as_deref(), Some("us"));
        assert_eq!(read(r"plugins\LSPD First Response.dll").as_deref(), Some("core"));

        // Reloading keeps it all.
        let db = Db::load(&root.join("data"));
        assert_eq!(db.state(&game).active.as_deref(), Some("American"));
        assert_eq!(db.mods.iter().filter(|m| m.enabled).count(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_clashes() {
        let root = tmp("clash");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("loose.dll"), "x").unwrap();
        let mut db = Db::load(&root.join("data"));
        db.install(&game, &plan("Pack", &[(r"plugins\LSPDFR\p.dll", "1")]), "p", false).unwrap();
        let clashes = db.clashes(&game, &plan("Other", &[(r"plugins\LSPDFR\p.dll", "2"), ("loose.dll", "y"), ("new.dll", "z")]));
        assert_eq!(clashes, [
            Clash { name: "Pack".into(), files: 1, libraries_only: false },
            Clash { name: String::new(), files: 1, libraries_only: false },
        ]);
        assert!(db.clashes(&game, &plan("Pack", &[(r"plugins\LSPDFR\p.dll", "2")])).is_empty(), "an update isn't a clash");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn never_touches_the_games_own_files() {
        assert!(protected(Path::new("x64a.rpf")).is_some());
        assert!(protected(Path::new(r"update\update.rpf")).is_some());
        assert!(protected(Path::new("GTA5.exe")).is_some());
        assert!(protected(Path::new(r"..\evil.dll")).is_some());
        assert!(protected(Path::new(r"LSPDFR Mod Installer\x.dll")).is_some());
        assert!(protected(Path::new(r"plugins\LSPDFR\x.dll")).is_none());
        assert!(protected(Path::new(r"lml\car\dlc.rpf")).is_none());
    }

    #[test]
    fn startup_script_keeps_other_lines_and_only_drops_its_own() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let text = "LoadPlugin \"LSPD First Response.dll\"\r\n";
        let (out, owned) = startup_lines(text, &s(&["LSPD First Response.dll", "Lights.dll"]), &[]);
        assert_eq!(out, "LoadPlugin \"LSPD First Response.dll\"\r\nLoadPlugin \"Lights.dll\"\r\n");
        assert_eq!(owned, ["Lights.dll"]);
        // Nothing to do: the file is left exactly as it was.
        assert_eq!(startup_lines(&out, &s(&["lights.dll"]), &owned).0, out);
        // Off again: only the line this app added goes.
        let (out, owned) = startup_lines(&out, &[], &owned);
        assert_eq!(out, text);
        assert!(owned.is_empty());
        assert_eq!(startup_lines("", &s(&["A.dll"]), &[]).0, "LoadPlugin \"A.dll\"\r\n");
        assert_eq!(loads("  loadplugin   \"My Plugin.dll\" "), Some("My Plugin.dll"));
    }

    fn one_part(name: &str, kind: Kind, actions: Vec<Action>) -> Plan {
        Plan {
            name: name.into(),
            exclusive: false,
            warnings: vec![],
            sets: vec![Set { label: name.into(), selected: true, skipped: vec![], parts: vec![Part { kind, label: name.into(), actions }] }],
        }
    }

    #[test]
    fn never_downgrades_a_library() {
        let root = tmp("versions");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        let newer = crate::dotnet::tests::versioned([1, 9, 3, 0]);
        fs::write(game.join("RAGENativeUI.dll"), &newer).unwrap();
        let old = root.join("RAGENativeUI.dll");
        fs::write(&old, crate::dotnet::tests::versioned([1, 8, 1, 0])).unwrap();
        let mut db = Db::load(&root.join("data"));
        let plan = one_part("EUP Menu", Kind::GameFiles, vec![Action::Copy { from: old, to: PathBuf::from("RAGENativeUI.dll") }]);
        let (m, notes) = db.install(&game, &plan, "test", false).unwrap();
        assert_eq!(fs::read(game.join("RAGENativeUI.dll")).unwrap(), newer);
        assert!(notes.iter().any(|n| n.contains("1.9.3") && n.contains("1.8.1")));
        // It isn't the mod's file, so removing the mod leaves it.
        db.uninstall(m.id).unwrap();
        assert!(game.join("RAGENativeUI.dll").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn eup_pack_outfits_are_in_eup_menu_while_the_pack_is_on() {
        let root = tmp("presets");
        let game = root.join("game");
        fs::create_dir_all(game.join(r"plugins\EUP")).unwrap();
        let base = "[Male LSPD Class A]\r\nGender=Male\r\nCategory=EUP\r\nCategory2=LSPD\r\n";
        fs::write(game.join(eup::PRESETS), base).unwrap();
        let mut db = Db::load(&root.join("data"));
        let outfits = "[Male Met Police TSG Soft]\r\nGender=Male\r\nCategory=EUP\r\nCategory2=Met Police\r\n";
        let plan = one_part("Met Police EUP", Kind::EupAddon, vec![Action::Write { to: PathBuf::from(eup::PACKS).join("Met Police.ini"), contents: outfits.into() }]);
        let presets = || fs::read_to_string(game.join(eup::PRESETS)).unwrap();
        let (m, notes) = db.install(&game, &plan, "test", false).unwrap();
        assert!(presets().starts_with(base) && presets().contains(outfits));
        assert!(notes.iter().any(|n| n.contains("Met Police section")));
        db.set_enabled(m.id, false).unwrap();
        assert_eq!(presets(), base);
        db.set_enabled(m.id, true).unwrap();
        assert!(presets().contains(outfits));
        db.set_clean(&game, true).unwrap();
        assert_eq!(presets(), base);
        db.set_clean(&game, false).unwrap();
        db.uninstall(m.id).unwrap();
        assert_eq!(presets(), base);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rph_plugins_load_at_startup_while_their_mod_is_on() {
        let root = tmp("startup");
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join(STARTUP), "LoadPlugin \"LSPD First Response.dll\"\r\n").unwrap();
        let src = root.join("Lights.dll");
        fs::write(&src, crate::dotnet::tests::fake_dll(&["", "RagePluginHook", "PluginAttribute"])).unwrap();
        let lib = root.join("Helper.dll");
        fs::write(&lib, b"MZ").unwrap();
        let mut db = Db::load(&root.join("data"));
        let plan = Plan {
            name: "Lights".into(),
            exclusive: false,
            warnings: vec![],
            sets: vec![Set { label: "Lights".into(), selected: true, skipped: vec![], parts: vec![Part {
                kind: Kind::RphPlugin,
                label: "Lights".into(),
                actions: vec![
                    Action::Copy { from: src.clone(), to: PathBuf::from(r"plugins\Lights.dll") },
                    Action::Copy { from: lib.clone(), to: PathBuf::from(r"plugins\Helper.dll") },
                    Action::Write { to: PathBuf::from(r"plugins\Lights.ini"), contents: "[x]".into() },
                ],
            }] }],
        };
        let script = || fs::read_to_string(game.join(STARTUP)).unwrap();
        let both = "LoadPlugin \"LSPD First Response.dll\"\r\nLoadPlugin \"Lights.dll\"\r\n";
        let (m, notes) = db.install(&game, &plan, "test", false).unwrap();
        assert_eq!(script(), both);
        assert!(notes.iter().any(|n| n.contains("Lights.dll")));
        db.set_enabled(m.id, false).unwrap();
        assert_eq!(script(), "LoadPlugin \"LSPD First Response.dll\"\r\n");
        db.set_enabled(m.id, true).unwrap();
        assert_eq!(script(), both);
        // An update keeps the one line.
        let (m, _) = db.install(&game, &plan, "test", false).unwrap();
        assert_eq!(script(), both);
        db.set_clean(&game, true).unwrap();
        assert_eq!(script(), "LoadPlugin \"LSPD First Response.dll\"\r\n");
        db.set_clean(&game, false).unwrap();
        assert_eq!(script(), both);
        db.uninstall(m.id).unwrap();
        assert_eq!(script(), "LoadPlugin \"LSPD First Response.dll\"\r\n");
        let _ = fs::remove_dir_all(&root);
    }
}
