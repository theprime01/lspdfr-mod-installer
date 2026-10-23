use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};

use crate::analyze::{Action, Kind, Plan};
use crate::game::{self, PREREQS};
use crate::install::{self, Db};
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
    Log,
}

pub struct App {
    settings: Settings,
    jobs: Jobs,
    tab: Tab,
    build: Option<u32>,
    confirm_remove: Option<u64>,
    filter: String,
}

enum Do {
    Install(usize),
    InstallSet(usize, usize),
    Select(usize, usize, bool),
    Dismiss(usize),
}

impl App {
    pub fn new(cc: &eframe::CreationContext, dropped: Vec<PathBuf>) -> App {
        let mut settings = Settings::load();
        if settings.game.as_deref().is_none_or(|g| !game::is_game(g)) {
            settings.game = game::find();
            settings.save();
        }
        let ctx = cc.egui_ctx.clone();
        let jobs = Jobs::new(Db::load(&install::data_dir()), move || ctx.request_repaint());
        let build = settings.game.as_deref().and_then(game::build);
        let mut app = App { settings, jobs, tab: Tab::Install, build, confirm_remove: None, filter: String::new() };
        app.drop_paths(dropped);
        app
    }

    fn game(&self) -> Option<PathBuf> {
        self.settings.game.clone()
    }

    fn set_game(&mut self, dir: PathBuf) {
        if game::is_game(&dir) {
            self.build = game::build(&dir);
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

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("LSPDFR Mod Installer");
            ui.add_space(12.0);
            match self.game() {
                Some(g) => {
                    let build = self.build.map(|b| format!("  (build {b})")).unwrap_or_default();
                    ui.label(format!("{}{build}", g.display()));
                    if ui.small_button("Open folder").clicked() {
                        open(&g.display().to_string());
                    }
                }
                None => {
                    ui.colored_label(RED, "GTA V not found.");
                }
            }
            if ui.small_button("Change...").clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title("Choose your GTA V folder").pick_folder() {
                    self.set_game(dir);
                }
            }
        });
        if let Some(g) = self.game() {
            if game::is_enhanced(&g) {
                ui.colored_label(RED, "This is GTA V Enhanced. LSPDFR only runs on GTA V Legacy.");
            }
        }
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
            ui.selectable_value(&mut self.tab, Tab::Log, "Log");
        });
    }

    fn install_tab(&mut self, ui: &mut egui::Ui) {
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let stroke = if hovering { egui::Stroke::new(2.0_f32, GREEN) } else { egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color) };
        egui::Frame::none().stroke(stroke).rounding(8.0).inner_margin(20.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if hovering { "Let go to install" } else { "Drop mods here" }).size(24.0).strong());
                ui.label("Zip, RAR or 7z downloads, folders, or single files: plugins, callouts, cars, scripts, ELS files, LSPDFR itself.");
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
                        let (text, color) = match &job.state {
                            State::Reading => ("Reading...".to_string(), AMBER),
                            State::Ready if job.plan.as_ref().is_some_and(|p| p.is_empty()) => ("Nothing to install".to_string(), RED),
                            State::Ready if job.plan.as_ref().is_some_and(|p| p.needs_decision()) => ("Pick a version below".to_string(), AMBER),
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
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.filter);
        });
        ui.add_space(4.0);
        let Ok(db) = self.jobs.db.try_lock() else {
            ui.label("Installing...");
            return;
        };
        let filter = self.filter.to_lowercase();
        let mods: Vec<install::Mod> = db.mods.iter().rev().filter(|m| m.game == game && m.name.to_lowercase().contains(&filter)).cloned().collect();
        drop(db);
        if mods.is_empty() {
            ui.label(RichText::new("No mods installed with this app yet.").weak());
        }
        let mut remove = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for m in &mods {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&m.name).strong());
                        let kinds: Vec<&str> = m.kinds.iter().map(|k| k.label()).collect();
                        ui.label(RichText::new(kinds.join(", ")).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.confirm_remove == Some(m.id) {
                                if ui.button(RichText::new("Yes, remove it").color(RED)).clicked() {
                                    remove = Some(m.id);
                                }
                                if ui.small_button("Cancel").clicked() {
                                    self.confirm_remove = None;
                                }
                            } else if ui.button("Remove").clicked() {
                                self.confirm_remove = Some(m.id);
                            }
                            ui.label(RichText::new(date(m.installed)).weak());
                        });
                    });
                    egui::CollapsingHeader::new(format!("{} files", m.files.len())).id_salt(("files", m.id)).show(ui, |ui| {
                        for f in &m.files {
                            let restored = if f.backup.is_some() { "  (replaced a file, which comes back on remove)" } else { "" };
                            ui.label(RichText::new(format!("{}{restored}", f.path.display())).monospace().small());
                        }
                    });
                });
            }
        });
        if let Some(id) = remove {
            self.confirm_remove = None;
            self.jobs.uninstall(id);
        }
    }

    fn setup_tab(&mut self, ui: &mut egui::Ui) {
        let Some(game) = self.game() else {
            ui.label("Choose your GTA V folder first (Change... at the top).");
            return;
        };
        ui.label("What LSPDFR needs. Download what's missing, then drop the download on the Install tab.");
        ui.add_space(6.0);
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
                if ui.small_button("Download page").clicked() {
                    open(p.url);
                }
                ui.end_row();
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Tips").strong());
        ui.label("Start the game with RAGEPluginHook.exe in the game folder, not from Steam, to play LSPDFR.");
        ui.label("Story mode mods don't run with BattlEye on: turn it off in the Rockstar Games Launcher settings for GTA V.");
        ui.label("Cars go into the lml folder as Lenny's Mod Loader packages. Its Mod Manager can switch them on and off.");
        if let Some(b) = self.build {
            ui.label(format!("Your game is build {b}. RAGE Plugin Hook has to support this build: after a GTA update, wait for an LSPDFR or RPH update."));
        }
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for line in &self.jobs.shared.lock().unwrap().log {
                ui.label(RichText::new(line).monospace());
            }
        });
    }
}

/// Warns about missing essentials this mod needs.
fn needs(ui: &mut egui::Ui, plan: &Plan, game: &Path) {
    let mut missing: Vec<&str> = Vec::new();
    for kind in plan.kinds() {
        for &need in game::needs(kind) {
            let p = game::prereq(need);
            if !game::has(game, need) && !missing.contains(&p.name) && !provides(plan, need) {
                missing.push(p.name);
            }
        }
    }
    if !missing.is_empty() {
        ui.colored_label(AMBER, format!("Needs {} to work in game, see Setup.", missing.join(", ")));
    }
}

/// True when the mod itself brings this essential (e.g. LSPDFR brings RPH).
fn provides(plan: &Plan, need: game::Need) -> bool {
    let file = match need {
        game::Need::Rph => "ragepluginhook.exe",
        game::Need::Lspdfr => "lspd first response.dll",
        game::Need::ScriptHookV => "scripthookv.dll",
        game::Need::AsiLoader => "dinput8.dll",
        game::Need::Shvdn => "scripthookvdotnet.asi",
        game::Need::Lml => "vfs.dll",
        game::Need::OpenIvAsi => "openiv.asi",
        game::Need::Els => "els.asi",
        game::Need::RageNativeUi => "ragenativeui.dll",
    };
    plan.parts().flat_map(|p| &p.actions).any(|a| a.to().file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(file)))
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
            Tab::Log => self.log_tab(ui),
        });
        // Keep the busy states moving.
        if self.jobs.shared.lock().unwrap().jobs.iter().any(|j| matches!(j.state, State::Reading | State::Installing)) {
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
