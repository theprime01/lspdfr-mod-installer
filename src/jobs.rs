//! Background work: unpacking and sorting each dropped mod, then installing
//! it, so the window never freezes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::analyze::{self, Plan};
use crate::extract;
use crate::game;
use crate::install::Db;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Reading,
    Ready,
    Installing,
    Installed,
    Failed(String),
}

pub struct Job {
    pub id: usize,
    pub source: PathBuf,
    pub state: State,
    pub plan: Option<Plan>,
    pub notes: Vec<String>,
    /// Staging folder to delete once done (None for dropped folders).
    staging: Option<PathBuf>,
}

impl Job {
    pub fn name(&self) -> String {
        self.plan.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| mod_name(&self.source))
    }
}

#[derive(Default)]
pub struct Shared {
    pub jobs: Vec<Job>,
    pub log: Vec<String>,
    next: usize,
}

/// The job list and the installed-mods database. The database has its own
/// lock, held while files are copied, so the window can still draw the jobs.
#[derive(Clone)]
pub struct Jobs {
    pub shared: Arc<Mutex<Shared>>,
    pub db: Arc<Mutex<Db>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

pub fn staging_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("LSPDFR Mod Installer").join("staging")
}

/// A readable mod name from a file or folder name.
pub fn mod_name(path: &Path) -> String {
    let raw = if path.is_dir() { path.file_name() } else { path.file_stem() };
    let raw = raw.map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Mod".into());
    let spaced: String = raw.chars().map(|c| if c == '_' { ' ' } else { c }).collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Jobs {
    pub fn new(db: Db, repaint: impl Fn() + Send + Sync + 'static) -> Jobs {
        let _ = fs::remove_dir_all(staging_root());
        Jobs { shared: Arc::new(Mutex::new(Shared::default())), db: Arc::new(Mutex::new(db)), repaint: Arc::new(repaint) }
    }

    pub fn log(&self, line: impl Into<String>) {
        self.shared.lock().unwrap().log.push(line.into());
        (self.repaint)();
    }

    fn with_job(&self, id: usize, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.shared.lock().unwrap().jobs.iter_mut().find(|j| j.id == id) {
            f(job);
        }
        (self.repaint)();
    }

    /// Starts reading a dropped file or folder. When `auto` is set and
    /// nothing needs a decision, it is installed straight away.
    pub fn add(&self, source: PathBuf, game: Option<PathBuf>, auto: bool) {
        let id = {
            let mut s = self.shared.lock().unwrap();
            s.next += 1;
            let id = s.next;
            s.jobs.insert(0, Job { id, source: source.clone(), state: State::Reading, plan: None, notes: Vec::new(), staging: None });
            id
        };
        (self.repaint)();
        let this = self.clone();
        std::thread::spawn(move || {
            let result = prepare(&source, id);
            let mut ready = false;
            this.with_job(id, |job| match result {
                Ok((plan, staging)) => {
                    job.staging = staging;
                    ready = auto && !plan.needs_decision();
                    job.plan = Some(plan);
                    job.state = State::Ready;
                }
                Err(e) => job.state = State::Failed(e),
            });
            if let (true, Some(game)) = (ready, game) {
                this.install(id, game);
            }
        });
    }

    pub fn install(&self, id: usize, game: PathBuf) {
        let mut work = None;
        self.with_job(id, |job| {
            if job.state == State::Ready && job.plan.is_some() {
                job.state = State::Installing;
                work = Some((job.plan.clone().unwrap(), job.source.display().to_string()));
            }
        });
        let Some((plan, source)) = work else { return };
        let this = self.clone();
        std::thread::spawn(move || {
            let outcome = if game::running() {
                Err("Close GTA V and RAGE Plugin Hook first, the game locks its files while it runs.".to_string())
            } else {
                this.db.lock().unwrap().install(&game, &plan, &source)
            };
            let line = match &outcome {
                Ok((m, _)) => format!("Installed {}: {} files", plan.name, m.files.len()),
                Err(e) => format!("Couldn't install {}: {e}", plan.name),
            };
            this.log(line);
            this.with_job(id, |job| match outcome {
                Ok((_, notes)) => {
                    job.notes = notes;
                    job.state = State::Installed;
                }
                Err(e) => {
                    // Back to ready so it can be tried again.
                    job.notes = vec![e];
                    job.state = State::Ready;
                }
            });
        });
    }

    /// Installs an extra set (one left out by default) into the same mod.
    pub fn install_set(&self, id: usize, set: usize, game: PathBuf) {
        let mut work = None;
        self.with_job(id, |job| {
            if let Some(plan) = &job.plan {
                if let Some(extra) = plan.sets.get(set) {
                    work = Some((plan.name.clone(), extra.clone(), job.source.display().to_string()));
                }
            }
        });
        let Some((name, extra, source)) = work else { return };
        if game::running() {
            return self.log("Close GTA V and RAGE Plugin Hook first.");
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let parts = extra.parts.iter().map(|p| (p.kind, p.actions.as_slice()));
            let outcome = this.db.lock().unwrap().add(&game, &name, &source, parts, Vec::new());
            match outcome {
                Ok((_, notes)) => {
                    this.log(format!("Added {} to {name}", extra.label));
                    this.with_job(id, |job| {
                        job.notes.extend(notes);
                        if let Some(s) = job.plan.as_mut().and_then(|p| p.sets.get_mut(set)) {
                            s.selected = true;
                        }
                    });
                }
                Err(e) => this.log(format!("Couldn't add {}: {e}", extra.label)),
            }
        });
    }

    pub fn uninstall(&self, id: u64) {
        if game::running() {
            return self.log("Close GTA V and RAGE Plugin Hook before removing mods.");
        }
        let (name, result) = {
            let mut db = self.db.lock().unwrap();
            let name = db.mods.iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default();
            (name, db.uninstall(id))
        };
        self.log(match result {
            Ok(()) => format!("Removed {name}"),
            Err(e) => format!("Couldn't remove {name}: {e}"),
        });
        let mut s = self.shared.lock().unwrap();
        for job in s.jobs.iter_mut().filter(|j| j.state == State::Installed && j.name().eq_ignore_ascii_case(&name)) {
            job.state = State::Ready;
            job.notes.clear();
        }
    }

    pub fn dismiss(&self, id: usize) {
        let mut s = self.shared.lock().unwrap();
        if let Some(i) = s.jobs.iter().position(|j| j.id == id) {
            let job = s.jobs.remove(i);
            if let Some(dir) = job.staging {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }
}

/// Unpacks (if needed) and analyses a dropped item.
fn prepare(source: &Path, id: usize) -> Result<(Plan, Option<PathBuf>), String> {
    let name = mod_name(source);
    if source.is_dir() {
        return Ok((analyze::analyze(source, &name), None));
    }
    if !source.is_file() {
        return Err("File not found".into());
    }
    let staging = staging_root().join(id.to_string());
    let _ = fs::remove_dir_all(&staging);
    if extract::is_archive(source) {
        extract::unpack(source, &staging).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!("Couldn't unpack it: {e}")
        })?;
    } else {
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let file = source.file_name().ok_or("No file name")?;
        fs::copy(source, staging.join(file)).map_err(|e| e.to_string())?;
    }
    Ok((analyze::analyze(&staging, &name), Some(staging)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_mods_from_their_downloads() {
        assert_eq!(mod_name(Path::new(r"C:\dl\Stop_The_Ped__4.9.zip")), "Stop The Ped 4.9");
        assert_eq!(mod_name(Path::new(r"C:\dl\2016 FPIU.rar")), "2016 FPIU");
    }
}
