use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};

use crate::analyze::{Action, Kind, Plan};
use crate::crash;
use crate::game::{self, PREREQS};
use crate::install::{self, Db, GameState};
use crate::jobs::{Jobs, State};
use crate::settings::Settings;

const GREEN: Color32 = Color32::from_rgb(90, 190, 110);
const AMBER: Color32 = Color32::from_rgb(230, 170, 60);
const RED: Color32 = Color32::from_rgb(225, 90, 80);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Install,
    Installed,
    Setup,
    Crash,
    Log,
}

/// A mod's settings file open in the editor.
struct Editor {
    path: PathBuf,
    text: String,
    status: String,
}

pub struct App {
    settings: Settings,
    jobs: Jobs,
    tab: Tab,
    build: Option<u32>,
    /// (build before, build now) after a GTA update.
    build_changed: Option<(u32, u32)>,
    confirm_remove: Option<u64>,
    filter: String,
    new_profile: Option<String>,
    confirm_delete_profile: Option<String>,
    editor: Option<Editor>,
    /// The last crash check: the log's date and what was found.
    crash: Option<(String, Vec<crash::Finding>)>,
}

enum Do {
    Install(usize),
    InstallSet(usize, usize),
    Select(usize, usize, bool),
    Dismiss(usize),
}

/// Changes asked for on the Installed and Crash help tabs.
enum Change {
    Enable(u64, bool),
    Profiles(u64, Option<Vec<String>>),
    Remove(u64),
    Edit(PathBuf),
}

impl App {
    pub fn new(cc: &eframe::CreationContext, dropped: Vec<PathBuf>) -> App {
        let mut settings = Settings::load();
        if settings.game.as_deref().is_none_or(|g| !game::is_game(g)) {
            settings.game = game::find();
        }
        let ctx = cc.egui_ctx.clone();
        let jobs = Jobs::new(Db::load(&install::data_dir()), move || ctx.request_repaint());
        let build = settings.game.as_deref().and_then(game::build);
        let build_changed = match (settings.known_build, build) {
            (Some(before), Some(now)) if before != now => Some((before, now)),
            _ => None,
        };
        if settings.known_build.is_none() {
            settings.known_build = build;
        }
        settings.save();
        jobs.check_update();
        let mut app = App {
            settings,
            jobs,
            tab: Tab::Install,
            build,
            build_changed,
            confirm_remove: None,
            filter: String::new(),
            new_profile: None,
            confirm_delete_profile: None,
            editor: None,
            crash: None,
        };
        app.drop_paths(dropped);
        app
    }

    fn game(&self) -> Option<PathBuf> {
        self.settings.game.clone()
    }

    fn set_game(&mut self, dir: PathBuf) {
        if game::is_game(&dir) {
            self.build = game::build(&dir);
            self.settings.known_build = self.build;
            self.build_changed = None;
            self.crash = None;
            self.settings.game = Some(dir);
            self.settings.save();
        } else {
            self.jobs.log(format!("{} doesn't contain GTA5.exe", dir.display()));
        }
    }

    fn drop_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.tab = Tab::Install;
        for p in paths {
            self.jobs.add(p, self.game(), self.settings.auto_install && self.game().is_some());
        }
    }

    /// Profiles and switches for the chosen game, if the database is free.
    fn state(&self) -> Option<GameState> {
        let game = self.game()?;
        self.jobs.db.try_lock().ok().map(|db| db.state(&game))
    }

    fn busy(&self) -> Option<String> {
        self.jobs.shared.lock().unwrap().busy.clone()
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("LSPDFR Mod Installer");
            ui.add_space(12.0);
            // Buttons from the right, then the game folder in what's left.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clean = self.state().is_some_and(|s| s.clean.is_some());
                if let Some(g) = self.game().filter(|g| game::has(g, game::Need::Rph) && !clean) {
                    if ui.button(RichText::new("Start LSPDFR").strong()).on_hover_text("Starts RAGEPluginHook.exe").clicked() {
                        match game::launch(&g) {
                            Ok(()) => self.jobs.log("Started RAGE Plugin Hook"),
                            Err(e) => self.jobs.log(format!("Couldn't start RAGE Plugin Hook: {e}")),
                        }
                    }
                    ui.add_space(8.0);
                }
                if ui.small_button("Change...").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().set_title("Choose your GTA V folder").pick_folder() {
                        self.set_game(dir);
                    }
                }
                match self.game() {
                    Some(g) => {
                        if ui.small_button("Open folder").clicked() {
                            open(&g.display().to_string());
                        }
                        let build = self.build.map(|b| format!("  (build {b})")).unwrap_or_default();
                        ui.add(egui::Label::new(format!("{}{build}", g.display())).truncate()).on_hover_text(g.display().to_string());
                    }
                    None => {
                        ui.colored_label(RED, "GTA V not found.");
                    }
                }
            });
        });
        if let Some(g) = self.game() {
            if game::is_enhanced(&g) {
                ui.colored_label(RED, "This is GTA V Enhanced. LSPDFR only runs on GTA V Legacy.");
            }
        }
        self.banners(ui);
        ui.add_space(2.0);
        self.profile_bar(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let installed = self.jobs.db.try_lock().map(|db| db.mods.iter().filter(|m| self.game().is_some_and(|g| m.game == g)).count()).ok();
            let missing = self.game().map(|g| PREREQS.iter().filter(|p| p.essential && !game::has(&g, p.need)).count()).unwrap_or(0);
            ui.selectable_value(&mut self.tab, Tab::Install, "Install");
            ui.selectable_value(&mut self.tab, Tab::Installed, match installed {
                Some(n) => format!("Installed ({n})"),
                None => "Installed".to_string(),
            });
            let setup = if missing > 0 { RichText::new(format!("Setup ({missing} missing)")).color(AMBER) } else { RichText::new("Setup") };
            ui.selectable_value(&mut self.tab, Tab::Setup, setup);
            let crashes = self.crash.as_ref().map_or(0, |(_, f)| f.len());
            let crash = if crashes > 0 { RichText::new(format!("Crash help ({crashes})")).color(RED) } else { RichText::new("Crash help") };
            ui.selectable_value(&mut self.tab, Tab::Crash, crash);
            ui.selectable_value(&mut self.tab, Tab::Log, "Log");
        });
    }

    /// Updates, game updates, the clean game and work in progress.
    fn banners(&mut self, ui: &mut egui::Ui) {
        let update = self.jobs.shared.lock().unwrap().update.clone();
        if let Some((version, url)) = update {
            ui.horizontal(|ui| {
                ui.colored_label(GREEN, format!("Version {version} of the installer is out (you have {}).", env!("CARGO_PKG_VERSION")));
                if self.busy().is_none() && ui.small_button("Update now").clicked() {
                    self.jobs.update(url);
                }
                if ui.small_button("What's new").clicked() {
                    open(&format!("https://github.com/{}/releases/latest", crate::net::REPO));
                }
            });
        }
        if let Some((before, now)) = self.build_changed {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(AMBER, format!("GTA V updated from build {before} to {now}. RAGE Plugin Hook, LSPDFR and Script Hook V usually stop working until they get an update for the new build."));
                if ui.small_button("Got it").clicked() {
                    self.build_changed = None;
                    self.settings.known_build = Some(now);
                    self.settings.save();
                }
            });
        }
        if let Some(busy) = self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(AMBER, busy);
            });
        }
    }

    fn profile_bar(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game() else { return };
        let Some(state) = self.state() else { return };
        let busy = self.busy().is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Profile:").strong());
            let clean = state.clean.is_some();
            let mut choose: Option<Option<String>> = None;
            ui.add_enabled_ui(!busy, |ui| {
                if ui.selectable_label(!clean && state.active.is_none(), "All mods").on_hover_text("Every installed mod on").clicked() {
                    choose = Some(None);
                }
                for p in &state.profiles {
                    let on = !clean && state.active.as_deref() == Some(p.as_str());
                    let r = ui.selectable_label(on, p).on_hover_text("Switch to this profile. Right-click to delete it.");
                    if r.clicked() {
                        choose = Some(Some(p.clone()));
                    }
                    r.context_menu(|ui| {
                        if ui.button(format!("Delete the {p} profile")).clicked() {
                            self.confirm_delete_profile = Some(p.clone());
                            ui.close_menu();
                        }
                    });
                }
            });
            if let Some(p) = choose {
                let name = p.clone().unwrap_or_else(|| "All mods".into());
                let g = game.clone();
                self.jobs.switch(format!("Switching to {name}..."), format!("Switched to {name}"), move |db| db.use_profile(&g, p));
            }
            match &mut self.new_profile {
                Some(name) => {
                    let r = ui.add(egui::TextEdit::singleline(name).hint_text("Profile name").desired_width(140.0));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.small_button("Add").clicked() || enter) && !name.trim().is_empty() {
                        let name = name.trim().to_string();
                        self.new_profile = None;
                        self.add_profiles(&game, &[&name]);
                    } else if ui.small_button("Cancel").clicked() {
                        self.new_profile = None;
                    }
                    r.request_focus();
                }
                None => {
                    if ui.small_button("+ New profile").clicked() {
                        self.new_profile = Some(String::new());
                    }
                    if state.profiles.is_empty() && ui.small_button("Set up American and British").on_hover_text("Makes an American and a British profile. Mods you install go into the one that's on.").clicked() {
                        self.add_profiles(&game, &["American", "British"]);
                    }
                }
            }
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| {
                let text = if clean { RichText::new("Clean game: ON").color(GREEN).strong() } else { RichText::new("Clean game: off") };
                let tip = if clean { "Puts your mods back" } else { "Switches every mod off, for GTA Online or story mode. Nothing is deleted." };
                if ui.selectable_label(clean, text).on_hover_text(tip).clicked() {
                    let g = game.clone();
                    let (busy, done) = if clean { ("Putting your mods back...", "Clean game off: mods are back") } else { ("Switching every mod off...", "Clean game on: every mod is off") };
                    self.jobs.switch(busy, done, move |db| db.set_clean(&g, !clean));
                }
            });
        });
        if let Some(name) = self.confirm_delete_profile.clone() {
            ui.horizontal(|ui| {
                ui.colored_label(AMBER, format!("Delete the {name} profile? Its mods stay installed, and are on under All mods."));
                if ui.button(RichText::new("Delete it").color(RED)).clicked() {
                    self.confirm_delete_profile = None;
                    let g = game.clone();
                    self.jobs.switch("Deleting the profile...", format!("Deleted the {name} profile"), move |db| db.remove_profile(&g, &name));
                }
                if ui.small_button("Cancel").clicked() {
                    self.confirm_delete_profile = None;
                }
            });
        }
        if let Some(was_on) = &state.clean {
            ui.colored_label(GREEN, format!("Clean game is on: {} mod{} installed with this app are switched off. Mods you installed by hand are still there.", was_on.len(), if was_on.len() == 1 { "" } else { "s" }));
        }
    }

    fn add_profiles(&mut self, game: &Path, names: &[&str]) {
        let mut db = self.jobs.db.lock().unwrap();
        for n in names {
            if let Err(e) = db.add_profile(game, n) {
                drop(db);
                return self.jobs.log(format!("Couldn't add the profile: {e}"));
            }
        }
    }

    fn install_tab(&mut self, ui: &mut egui::Ui) {
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let stroke = if hovering { egui::Stroke::new(2.0_f32, GREEN) } else { egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color) };
        let state = self.state();
        egui::Frame::none().stroke(stroke).rounding(8.0).inner_margin(20.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if hovering { "Let go to install" } else { "Drop mods here" }).size(24.0).strong());
                ui.label("Zip, RAR or 7z downloads, folders, or single files: plugins, callouts, cars, scripts, ELS files, LSPDFR itself.");
                if let Some(p) = state.as_ref().and_then(|s| s.active.clone()) {
                    ui.label(RichText::new(format!("New mods go into the {p} profile (LSPDFR, RPH and other essentials go into every profile).")).weak());
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 230.0_f32).max(0.0) / 2.0);
                    if ui.button("Choose files...").clicked() {
                        if let Some(files) = rfd::FileDialog::new().add_filter("Mods", &["zip", "rar", "7z", "dll", "asi", "rpf", "xml", "yft", "ytd", "cs", "vb", "oiv"]).add_filter("All files", &["*"]).pick_files() {
                            self.drop_paths(files);
                        }
                    }
                    if ui.button("Choose a folder...").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.drop_paths(vec![dir]);
                        }
                    }
                });
            });
        });
        ui.checkbox(&mut self.settings.auto_install, "Install straight away unless there's a choice to make").changed().then(|| self.settings.save());
        ui.add_space(6.0);

        let game = self.game();
        let mut actions = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let shared = self.jobs.shared.lock().unwrap();
            if shared.jobs.is_empty() {
                ui.label(RichText::new("Nothing dropped yet.").weak());
            }
            for job in &shared.jobs {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(job.name()).strong().size(16.0));
                        let replaces_mod = job.clashes.iter().any(|c| !c.name.is_empty() && !c.libraries_only);
                        let (text, color) = match &job.state {
                            State::Reading => ("Reading...".to_string(), AMBER),
                            State::Ready if job.plan.as_ref().is_some_and(|p| p.is_empty()) => ("Nothing to install".to_string(), RED),
                            State::Ready if job.plan.as_ref().is_some_and(|p| p.needs_decision()) => ("Pick a version below".to_string(), AMBER),
                            State::Ready if replaces_mod => ("Replaces another mod's files, check below".to_string(), AMBER),
                            State::Ready => ("Ready".to_string(), AMBER),
                            State::Installing => ("Installing...".to_string(), AMBER),
                            State::Installed => ("Installed".to_string(), GREEN),
                            State::Failed(e) => (e.clone(), RED),
                        };
                        ui.colored_label(color, text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !matches!(job.state, State::Reading | State::Installing) && ui.small_button("Dismiss").clicked() {
                                actions.push(Do::Dismiss(job.id));
                            }
                            let can = job.state == State::Ready && job.plan.as_ref().is_some_and(|p| p.parts().next().is_some());
                            if can && game.is_some() && ui.button(RichText::new("Install").strong()).clicked() {
                                actions.push(Do::Install(job.id));
                            }
                        });
                    });
                    ui.label(RichText::new(job.source.display().to_string()).weak().small());
                    for n in &job.notes {
                        ui.colored_label(if job.state == State::Installed { Color32::GRAY } else { RED }, n);
                    }
                    if job.state != State::Installed {
                        clashes(ui, &job.clashes);
                    }
                    if let Some(plan) = &job.plan {
                        if let Some(g) = &game {
                            needs(ui, plan, g);
                        }
                        plan_view(ui, job.id, plan, &job.state, &mut actions);
                    }
                });
                ui.add_space(4.0);
            }
        });

        for a in actions {
            match a {
                Do::Install(id) => {
                    if let Some(g) = &game {
                        self.jobs.install(id, g.clone());
                    }
                }
                Do::InstallSet(id, set) => {
                    if let Some(g) = &game {
                        self.jobs.install_set(id, set, g.clone());
                    }
                }
                Do::Select(id, set, on) => {
                    if let Some(job) = self.jobs.shared.lock().unwrap().jobs.iter_mut().find(|j| j.id == id) {
                        if let Some(p) = job.plan.as_mut() {
                            p.select(set, on);
                        }
                    }
                }
                Do::Dismiss(id) => self.jobs.dismiss(id),
            }
        }
    }

    fn installed_tab(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game() else {
            ui.label("Choose your GTA V folder first.");
            return;
        };
        let Ok(db) = self.jobs.db.try_lock() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working...");
            });
            return;
        };
        let state = db.state(&game);
        let all: Vec<install::Mod> = db.mods.iter().filter(|m| m.game == game).cloned().collect();
        drop(db);
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.filter);
            let on = all.iter().filter(|m| m.enabled).count();
            ui.label(RichText::new(format!("{on} of {} on", all.len())).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !all.is_empty() && ui.button("Export list...").clicked() {
                    export(&all, &state, &mut self.jobs);
                }
            });
        });
        ui.add_space(4.0);
        let filter = self.filter.to_lowercase();
        let mods: Vec<&install::Mod> = all.iter().rev().filter(|m| m.name.to_lowercase().contains(&filter) || m.models.iter().any(|n| n.to_lowercase().contains(&filter))).collect();
        if mods.is_empty() {
            ui.label(RichText::new("No mods installed with this app yet.").weak());
        }
        let busy = self.busy().is_some();
        let mut changes = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for m in mods {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut on = m.enabled;
                        if ui.add_enabled(!busy, egui::Checkbox::without_text(&mut on)).on_hover_text(if on { "On. Untick to switch it off without removing it." } else { "Off. Tick to switch it back on." }).changed() {
                            changes.push(Change::Enable(m.id, on));
                        }
                        let name = RichText::new(&m.name).strong();
                        ui.label(if m.enabled { name } else { name.weak().strikethrough() });
                        if !m.enabled {
                            ui.label(RichText::new("off").color(AMBER));
                        }
                        let kinds: Vec<&str> = m.kinds.iter().map(|k| k.label()).collect();
                        ui.label(RichText::new(kinds.join(", ")).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.confirm_remove == Some(m.id) {
                                if ui.button(RichText::new("Yes, remove it").color(RED)).clicked() {
                                    changes.push(Change::Remove(m.id));
                                }
                                if ui.small_button("Cancel").clicked() {
                                    self.confirm_remove = None;
                                }
                            } else if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                                self.confirm_remove = Some(m.id);
                            }
                            ui.label(RichText::new(date(m.installed)).weak());
                        });
                    });
                    if !state.profiles.is_empty() {
                        profile_chips(ui, m, &state.profiles, busy, &mut changes);
                    }
                    if !m.models.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Spawn:");
                            for n in &m.models {
                                if ui.add(egui::Label::new(RichText::new(n).monospace().color(Color32::from_rgb(200, 140, 230))).sense(egui::Sense::click())).on_hover_text("Click to copy").clicked() {
                                    ui.ctx().copy_text(n.clone());
                                }
                            }
                            if m.models.len() > 1 && ui.small_button("Copy all").clicked() {
                                ui.ctx().copy_text(m.models.join("\n"));
                            }
                        });
                    }
                    let configs = settings_files(m);
                    if !configs.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Settings:");
                            for f in configs {
                                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                if ui.small_button(format!("Edit {name}")).on_hover_text(f.display().to_string()).clicked() {
                                    changes.push(Change::Edit(game.join(f)));
                                }
                            }
                        });
                    }
                    egui::CollapsingHeader::new(format!("{} file{}", m.files.len(), if m.files.len() == 1 { "" } else { "s" })).id_salt(("files", m.id)).show(ui, |ui| {
                        for f in &m.files {
                            let restored = if f.backup.is_some() { "  (replaced a file, which comes back on remove)" } else { "" };
                            ui.label(RichText::new(format!("{}{restored}", f.path.display())).monospace().small());
                        }
                    });
                });
            }
        });
        self.apply(changes, &all);
    }

    fn apply(&mut self, changes: Vec<Change>, mods: &[install::Mod]) {
        let name = |id: u64| mods.iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default();
        for c in changes {
            match c {
                Change::Enable(id, on) => {
                    let n = name(id);
                    let (busy, done) = if on { (format!("Switching {n} on..."), format!("Switched {n} on")) } else { (format!("Switching {n} off..."), format!("Switched {n} off")) };
                    self.jobs.switch(busy, done, move |db| db.set_enabled(id, on));
                }
                Change::Profiles(id, profiles) => {
                    let n = name(id);
                    self.jobs.switch(format!("Updating {n}..."), format!("Changed the profiles of {n}"), move |db| db.set_profiles(id, profiles));
                }
                Change::Remove(id) => {
                    self.confirm_remove = None;
                    self.jobs.uninstall(id);
                }
                Change::Edit(path) => match std::fs::read_to_string(&path) {
                    Ok(text) => self.editor = Some(Editor { path, text, status: String::new() }),
                    Err(e) => self.jobs.log(format!("Couldn't open {}: {e}", path.display())),
                },
            }
        }
    }

    fn setup_tab(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game() else {
            ui.label("Choose your GTA V folder first (Change... at the top).");
            return;
        };
        ui.label("What LSPDFR needs. Download what's missing, then drop the download on the Install tab. The ones hosted on GitHub install with one click.");
        ui.add_space(6.0);
        let busy = self.busy().is_some();
        let mut fetch = None;
        egui::Grid::new("prereqs").num_columns(4).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
            for p in PREREQS {
                let have = game::has(&game, p.need);
                ui.label(RichText::new(p.name).strong());
                match (have, p.essential) {
                    (true, _) => ui.colored_label(GREEN, "Installed"),
                    (false, true) => ui.colored_label(RED, "Missing"),
                    (false, false) => ui.colored_label(AMBER, "Not installed"),
                };
                ui.label(p.what);
                ui.horizontal(|ui| {
                    if let Some((repo, asset)) = p.github {
                        let text = if have { "Update" } else { "Install" };
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new(text).strong()).small()).on_hover_text(format!("Downloads {asset} from github.com/{repo} and installs it")).clicked() {
                            fetch = Some((repo, asset));
                        }
                    }
                    if ui.small_button("Download page").clicked() {
                        open(p.url);
                    }
                });
                ui.end_row();
            }
        });
        if let Some((repo, asset)) = fetch {
            self.jobs.fetch(repo, asset, Some(game.clone()));
            self.tab = Tab::Install;
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Tips").strong());
        ui.label("Start the game with Start LSPDFR at the top (RAGEPluginHook.exe), not from Steam, to play LSPDFR.");
        ui.label("Story mode mods don't run with BattlEye on: turn it off in the Rockstar Games Launcher settings for GTA V.");
        ui.label("For GTA Online, turn on Clean game at the top first, and back off afterwards.");
        ui.label("Cars go into the lml folder as Lenny's Mod Loader packages.");
        if let Some(b) = self.build {
            ui.label(format!("Your game is build {b}. RAGE Plugin Hook has to support this build: after a GTA update, wait for an LSPDFR or RPH update."));
        }
    }

    fn crash_tab(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game() else {
            ui.label("Choose your GTA V folder first.");
            return;
        };
        let log = crash::log_path(&game);
        if self.crash.is_none() {
            self.crash = Some(read_crashes(&log));
        }
        ui.label("Reads RagePluginHook.log from your last LSPDFR session and works out which mod crashed.");
        ui.horizontal(|ui| {
            if ui.button("Check again").clicked() {
                self.crash = Some(read_crashes(&log));
            }
            if log.is_file() && ui.small_button("Open the log").clicked() {
                let _ = std::process::Command::new("notepad").arg(&log).spawn();
            }
            if let Some((when, _)) = &self.crash {
                ui.label(RichText::new(when).weak());
            }
        });
        ui.add_space(6.0);
        let Some((_, findings)) = self.crash.clone() else { return };
        let mods: Vec<install::Mod> = match self.jobs.db.try_lock() {
            Ok(db) => db.mods.iter().filter(|m| m.game == game).cloned().collect(),
            Err(_) => Vec::new(),
        };
        if findings.is_empty() && log.is_file() {
            ui.colored_label(GREEN, "No crashes in the last session's log.");
        }
        let busy = self.busy().is_some();
        let mut changes = Vec::new();
        let mut fetch = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, f) in findings.iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&f.title).strong().color(RED));
                    if let Some(missing) = &f.missing {
                        let prereq = PREREQS.iter().find(|p| p.name.eq_ignore_ascii_case(missing) || p.github.is_some_and(|(_, a)| a.to_lowercase().starts_with(&missing.to_lowercase())));
                        match prereq.and_then(|p| p.github) {
                            Some((repo, asset)) => {
                                if ui.add_enabled(!busy, egui::Button::new(format!("Install the latest {missing}"))).clicked() {
                                    fetch = Some((repo, asset));
                                }
                            }
                            None => {
                                ui.label(format!("Install or update {missing}: it usually comes with the plugin's download, or see its page on lcpdfr.com."));
                            }
                        }
                    }
                    match crash::blame(f, &mods) {
                        Some(m) => {
                            ui.horizontal(|ui| {
                                ui.label(format!("From {} (installed {})", m.name, date(m.installed)));
                                if m.enabled && ui.add_enabled(!busy, egui::Button::new("Switch it off")).clicked() {
                                    changes.push(Change::Enable(m.id, false));
                                }
                                if self.confirm_remove == Some(m.id) {
                                    if ui.button(RichText::new("Yes, remove it").color(RED)).clicked() {
                                        changes.push(Change::Remove(m.id));
                                    }
                                } else if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                                    self.confirm_remove = Some(m.id);
                                }
                            });
                            ui.label(RichText::new("Check its page for an update, or that it suits your LSPDFR version, before putting it back.").weak());
                        }
                        None => match &f.culprit {
                            Some(c) => {
                                ui.label(RichText::new(format!("{c} wasn't installed with this app: look for {c}.dll in plugins\\LSPDFR or plugins.")).weak());
                            }
                            None => {
                                ui.label(RichText::new("The log doesn't say which plugin. Details has the error.").weak());
                            }
                        },
                    }
                    egui::CollapsingHeader::new("Details").id_salt(("crash", i)).show(ui, |ui| {
                        ui.label(RichText::new(&f.detail).monospace().small());
                    });
                });
                ui.add_space(4.0);
            }
        });
        if let Some((repo, asset)) = fetch {
            self.jobs.fetch(repo, asset, Some(game.clone()));
            self.tab = Tab::Install;
        }
        self.apply(changes, &mods);
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for line in &self.jobs.shared.lock().unwrap().log {
                ui.label(RichText::new(line).monospace());
            }
        });
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else { return };
        let mut open = true;
        let title = editor.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Window::new(format!("Editing {title}")).open(&mut open).default_size([720.0, 520.0]).show(ctx, |ui| {
            ui.label(RichText::new(editor.path.display().to_string()).weak().small());
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    editor.status = match std::fs::write(&editor.path, &editor.text) {
                        Ok(()) => "Saved. Restart the game (or reload the plugin) to use it.".into(),
                        Err(e) => format!("Couldn't save: {e}"),
                    };
                }
                ui.label(&editor.status);
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                if ui.add(egui::TextEdit::multiline(&mut editor.text).code_editor().desired_width(f32::INFINITY).desired_rows(28)).changed() {
                    editor.status = "Not saved".into();
                }
            });
        });
        if !open {
            self.editor = None;
        }
    }
}

/// Which profiles a mod is in, as ticks.
fn profile_chips(ui: &mut egui::Ui, m: &install::Mod, profiles: &[String], busy: bool, changes: &mut Vec<Change>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("In:").weak());
        ui.add_enabled_ui(!busy, |ui| {
            let mut every = m.profiles.is_none();
            if ui.checkbox(&mut every, "Every profile").on_hover_text("For things every setup needs, like LSPDFR itself").changed() {
                changes.push(Change::Profiles(m.id, if every { None } else { Some(Vec::new()) }));
            }
            ui.add_enabled_ui(!every, |ui| {
                for p in profiles {
                    let mut on = every || m.in_profile(Some(p));
                    if ui.checkbox(&mut on, p).changed() {
                        let mut list = m.profiles.clone().unwrap_or_default();
                        list.retain(|x| !x.eq_ignore_ascii_case(p));
                        if on {
                            list.push(p.clone());
                        }
                        changes.push(Change::Profiles(m.id, Some(list)));
                    }
                }
            });
        });
    });
}

/// A mod's settings files (.ini and friends), not the car files LML reads.
fn settings_files(m: &install::Mod) -> Vec<PathBuf> {
    if !m.enabled {
        return Vec::new();
    }
    m.files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| matches!(crate::extract::ext(p).as_str(), "ini" | "cfg" | "json" | "xml" | "toml"))
        .filter(|p| !p.starts_with("lml") && !p.to_string_lossy().to_lowercase().starts_with(r"els\"))
        // RAGENativeUI.xml next to RAGENativeUI.dll is code documentation.
        .filter(|p| crate::extract::ext(p) != "xml" || !m.files.iter().any(|f| f.path.with_extension("").as_os_str().eq_ignore_ascii_case(p.with_extension("").as_os_str()) && crate::extract::ext(&f.path) == "dll"))
        .take(8)
        .collect()
}

fn read_crashes(log: &Path) -> (String, Vec<crash::Finding>) {
    let Ok(text) = std::fs::read(log) else {
        return ("No RagePluginHook.log yet: start the game with Start LSPDFR first.".into(), Vec::new());
    };
    let when = std::fs::metadata(log).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| format!("Log from {}", date(d.as_secs()))).unwrap_or_default();
    (when, crash::check(&String::from_utf8_lossy(&text)))
}

fn export(mods: &[install::Mod], state: &GameState, jobs: &mut Jobs) {
    let Some(path) = rfd::FileDialog::new().set_file_name("LSPDFR mods.txt").add_filter("Text", &["txt"]).save_file() else { return };
    let mut out = format!("LSPDFR mods in {}\n\n", state.game.display());
    for m in mods {
        let kinds: Vec<&str> = m.kinds.iter().map(|k| k.label()).collect();
        out += &format!("{}{}  ({})  installed {}\n", m.name, if m.enabled { "" } else { "  [off]" }, kinds.join(", "), date(m.installed));
        if !state.profiles.is_empty() {
            out += &format!("    profiles: {}\n", m.profiles.as_ref().map_or("every profile".to_string(), |l| if l.is_empty() { "none".into() } else { l.join(", ") }));
        }
        if !m.models.is_empty() {
            out += &format!("    spawn: {}\n", m.models.join(", "));
        }
        out += &format!("    from: {}\n", m.source);
    }
    match std::fs::write(&path, out.replace('\n', "\r\n")) {
        Ok(()) => jobs.log(format!("Saved the list to {}", path.display())),
        Err(e) => jobs.log(format!("Couldn't save the list: {e}")),
    }
}

/// What an install would replace.
fn clashes(ui: &mut egui::Ui, clashes: &[install::Clash]) {
    for c in clashes {
        let files = format!("{} file{}", c.files, if c.files == 1 { "" } else { "s" });
        if c.name.is_empty() {
            ui.label(RichText::new(format!("Replaces {files} already in the game folder. They're backed up and come back if you remove this.")).weak());
        } else if c.libraries_only {
            ui.label(RichText::new(format!("Updates {files} shared with {} (libraries like RAGENativeUI, that's normal).", c.name)).weak());
        } else {
            ui.colored_label(AMBER, format!("Replaces {files} from {}. Switching this off or removing it puts them back.", c.name));
        }
    }
}

/// Warns about missing essentials this mod needs.
fn needs(ui: &mut egui::Ui, plan: &Plan, game: &Path) {
    let mut missing: Vec<&str> = Vec::new();
    for kind in plan.kinds() {
        for &need in game::needs(kind) {
            let p = game::prereq(need);
            if !game::has(game, need) && !missing.contains(&p.name) && !game::provides(plan, need) {
                missing.push(p.name);
            }
        }
    }
    if !missing.is_empty() {
        ui.colored_label(AMBER, format!("Needs {} to work in game, see Setup.", missing.join(", ")));
    }
}

fn plan_view(ui: &mut egui::Ui, id: usize, plan: &Plan, state: &State, actions: &mut Vec<Do>) {
    let editable = *state == State::Ready;
    let several = plan.sets.len() > 1;
    if several {
        ui.label(if plan.exclusive { "This download has different versions of the same mod. Pick one:" } else { "This download has several parts:" });
    }
    for (i, set) in plan.sets.iter().enumerate() {
        if several {
            ui.horizontal(|ui| {
                let mut on = set.selected;
                let changed = if plan.exclusive { ui.radio(on, &set.label).clicked() && { on = true; true } } else { ui.checkbox(&mut on, &set.label).changed() };
                if changed && editable {
                    actions.push(Do::Select(id, i, on));
                }
                if *state == State::Installed && !set.selected && !set.parts.is_empty() && !plan.exclusive && ui.small_button("Install this too").clicked() {
                    actions.push(Do::InstallSet(id, i));
                }
            });
        }
        if !set.selected && several {
            continue;
        }
        ui.indent(("set", id, i), |ui| {
            for (n, part) in set.parts.iter().enumerate() {
                let files = part.actions.iter().filter(|a| !a.to().as_os_str().is_empty()).count();
                let title = if part.label.is_empty() { part.kind.label().to_string() } else { format!("{}: {}", part.kind.label(), part.label) };
                egui::CollapsingHeader::new(RichText::new(format!("{title}  ({files} file{})", if files == 1 { "" } else { "s" })).color(kind_color(part.kind)))
                    .id_salt(("part", id, i, n))
                    .show(ui, |ui| {
                        for a in &part.actions {
                            let line = match a {
                                Action::Copy { to, .. } => to.display().to_string(),
                                Action::Write { to, .. } => format!("{}  (made by the installer)", to.display()),
                            };
                            ui.label(RichText::new(line).monospace().small());
                        }
                    });
            }
            if !set.skipped.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("Not installed ({})", set.skipped.len())).weak()).id_salt(("skip", id, i)).show(ui, |ui| {
                    for s in &set.skipped {
                        ui.label(RichText::new(format!("{}  -  {}", s.path.display(), s.reason)).small());
                    }
                });
            }
        });
    }
}

fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::LspdfrPlugin | Kind::RphPlugin => Color32::from_rgb(110, 160, 240),
        Kind::AddonVehicle | Kind::ReplaceVehicle | Kind::DlcPack | Kind::LmlMod => Color32::from_rgb(200, 140, 230),
        Kind::Els => Color32::from_rgb(240, 120, 120),
        _ => Color32::from_gray(200),
    }
}

fn open(target: &str) {
    let _ = std::process::Command::new("explorer").arg(target).spawn();
}

/// Unix time as a local-agnostic YYYY-MM-DD.
fn date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        self.drop_paths(dropped);
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            self.header(ui);
            ui.add_space(2.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Install => self.install_tab(ui),
            Tab::Installed => self.installed_tab(ui),
            Tab::Setup => self.setup_tab(ui),
            Tab::Crash => self.crash_tab(ui),
            Tab::Log => self.log_tab(ui),
        });
        self.editor_window(ctx);
        // Keep the busy states moving.
        let busy = self.busy().is_some() || self.jobs.shared.lock().unwrap().jobs.iter().any(|j| matches!(j.state, State::Reading | State::Installing));
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_dates() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(1_792_713_600), "2026-10-23");
    }
}
