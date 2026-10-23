#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analyze;
mod dotnet;
mod extract;
mod game;
mod install;
mod jobs;
mod lml;
mod settings;
mod ui;

fn main() -> eframe::Result<()> {
    // `--plan <file or folder>` prints where everything would go (debug builds show it).
    let args: Vec<String> = std::env::args().collect();
    if let [_, flag, path] = args.as_slice() {
        if flag == "--plan" {
            print_plan(std::path::Path::new(path));
            return Ok(());
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("LSPDFR Mod Installer")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([640.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    // Files dropped on the exe's icon arrive as arguments.
    let dropped: Vec<std::path::PathBuf> = args.iter().skip(1).map(std::path::PathBuf::from).collect();
    eframe::run_native("LSPDFR Mod Installer", options, Box::new(|cc| Ok(Box::new(ui::App::new(cc, dropped)))))
}

fn print_plan(source: &std::path::Path) {
    let name = jobs::mod_name(source);
    let staging = std::env::temp_dir().join("lspdfr-plan");
    let _ = std::fs::remove_dir_all(&staging);
    let root = if extract::is_archive(source) {
        if let Err(e) = extract::unpack(source, &staging) {
            return println!("Couldn't unpack: {e}");
        }
        staging.clone()
    } else {
        source.to_path_buf()
    };
    let plan = analyze::analyze(&root, &name);
    println!("{} (exclusive: {}, needs decision: {})", plan.name, plan.exclusive, plan.needs_decision());
    for set in &plan.sets {
        println!("[{}] {}", if set.selected { "x" } else { " " }, set.label);
        for part in &set.parts {
            println!("  {:?} {}", part.kind, part.label);
            for a in &part.actions {
                match a {
                    analyze::Action::Copy { to, .. } => println!("    {}", to.display()),
                    analyze::Action::Write { to, contents } => println!("    {} (written)\n{contents}", to.display()),
                }
            }
        }
        for s in &set.skipped {
            println!("  skip {} - {}", s.path.display(), s.reason);
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
}
