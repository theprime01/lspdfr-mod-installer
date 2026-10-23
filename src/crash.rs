//! Reads RAGE Plugin Hook's log from the last game session and works out
//! which plugin crashed, or which file a plugin was missing.

use std::path::{Path, PathBuf};

use crate::install::Mod;

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub title: String,
    pub detail: String,
    /// The plugin or library name the log blames.
    pub culprit: Option<String>,
    /// A missing file the plugin wanted, e.g. "RAGENativeUI".
    pub missing: Option<String>,
}

pub fn log_path(game: &Path) -> PathBuf {
    game.join("RagePluginHook.log")
}

/// Code that's never the one to blame: the game, RPH, LSPDFR and .NET.
const FRAMEWORK: &[&str] = &["system", "microsoft", "rage", "lspd_first_response", "lspdfr", "mono", "ragenativeui", "<module>", "albo1125", "newtonsoft", "naudio", "calloutinterfaceapi"];

pub fn check(log: &str) -> Vec<Finding> {
    let lines: Vec<&str> = log.lines().map(strip_time).collect();
    let mut out: Vec<Finding> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.contains("UNHANDLED EXCEPTION") || line.starts_with("Exception type:") {
            let (finding, next) = exception(&lines, i);
            if !out.contains(&finding) {
                out.push(finding);
            }
            i = next.max(i + 1);
            continue;
        }
        if let Some(name) = terminated(line) {
            if !out.iter().any(|f| f.culprit.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&name))) {
                out.push(Finding { title: format!("{name} was stopped by RAGE Plugin Hook"), detail: line.trim().to_string(), culprit: Some(name), missing: None });
            }
        }
        i += 1;
    }
    // A missing file is the real problem behind many crashes.
    for f in out.iter_mut() {
        if let Some(missing) = &f.missing {
            let who = f.culprit.clone().unwrap_or_else(|| "A plugin".into());
            f.title = format!("{who} needs {missing}.dll, which isn't installed or is the wrong version");
        }
    }
    out
}

/// Reads one exception report, from its first line. Returns the finding
/// and where the report ends.
fn exception(lines: &[&str], start: usize) -> (Finding, usize) {
    let mut origin = String::new();
    let mut kind = String::new();
    let mut message = String::new();
    let mut culprit = None;
    let mut missing = None;
    let mut end = lines.len();
    let mut in_trace = false;
    for (n, line) in lines.iter().enumerate().skip(start) {
        let line = line.trim();
        if n > start + 2 && (line.contains("UNHANDLED EXCEPTION") || (line.starts_with("Exception type:") && !kind.is_empty())) {
            end = n;
            break;
        }
        // The stack trace is the last thing in a report.
        if in_trace && !line.starts_with("at ") && !line.starts_with("---") {
            end = if line.starts_with("=====") { n + 1 } else { n };
            break;
        }
        in_trace |= line.starts_with("at ");
        if let Some(rest) = line.strip_prefix("Origin:") {
            origin = rest.trim().to_string();
            if culprit.is_none() {
                culprit = quoted_plugin(rest);
            }
        } else if let Some(rest) = line.strip_prefix("Exception type:") {
            kind = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Exception message:") {
            message = rest.trim().to_string();
            missing = missing.or_else(|| missing_assembly(rest));
        } else if let Some(frame) = line.strip_prefix("at ") {
            if let Some(ns) = namespace(frame) {
                if culprit.is_none() && !FRAMEWORK.contains(&ns.to_lowercase().as_str()) {
                    culprit = Some(ns);
                }
            }
        } else if missing.is_none() {
            missing = missing_assembly(line);
        }
        if line.starts_with("==========") && n > start + 3 && !kind.is_empty() {
            end = n + 1;
            break;
        }
    }
    let title = match &culprit {
        Some(c) => format!("{c} crashed"),
        None => "Something crashed".to_string(),
    };
    let mut detail = [kind, message].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": ");
    if !origin.is_empty() {
        detail += &format!("\nIn {origin}");
    }
    (Finding { title, detail, culprit, missing }, end)
}

/// Drops the `[10/23/2026 8:15:31 PM.123] ` RPH puts on every line.
fn strip_time(line: &str) -> &str {
    match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((time, rest)) if time.contains(':') => rest,
        _ => line,
    }
}

/// `Plugin "StopThePed" was terminated ...` or `... unloaded because ...`.
fn terminated(line: &str) -> Option<String> {
    let l = line.to_lowercase();
    if !(l.contains("was terminated") || l.contains("caused an exception") || l.contains("crashed")) || !l.contains("plugin") {
        return None;
    }
    quoted_plugin(line)
}

fn quoted_plugin(text: &str) -> Option<String> {
    let after = &text[text.find("Plugin \"").map(|i| i + 8).or_else(|| text.find("plugin \"").map(|i| i + 8))?..];
    let name = after.split(['"', '\\']).next()?.trim();
    (!name.is_empty() && !FRAMEWORK.contains(&name.to_lowercase().as_str()) && !name.eq_ignore_ascii_case("LSPD First Response")).then(|| name.to_string())
}

/// The first part of `StopThePed.API.Functions.Go()`.
fn namespace(frame: &str) -> Option<String> {
    let head = frame.split(['(', ' ']).next()?;
    let ns = head.split('.').next()?.trim();
    (!ns.is_empty()).then(|| ns.to_string())
}

/// `Could not load file or assembly 'RAGENativeUI, Version=...'`.
fn missing_assembly(text: &str) -> Option<String> {
    let i = text.find("Could not load file or assembly '")? + "Could not load file or assembly '".len();
    let name = text[i..].split([',', '\'']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The installed mod a finding points at: one with a .dll named like it.
pub fn blame<'a>(finding: &Finding, mods: &'a [Mod]) -> Option<&'a Mod> {
    let culprit = simple(finding.culprit.as_deref()?);
    if culprit.is_empty() {
        return None;
    }
    let dll = |m: &&Mod| m.files.iter().any(|f| {
        crate::extract::ext(&f.path) == "dll" && f.path.file_stem().is_some_and(|s| simple(&s.to_string_lossy()) == culprit)
    });
    mods.iter().rev().find(dll).or_else(|| mods.iter().rev().find(|m| simple(&m.name).contains(&culprit)))
}

fn simple(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"[10/23/2026 8:15:31 PM.123] LSPD First Response: Loading plugin StopThePed
[10/23/2026 8:20:02 PM.456] ==============================
[10/23/2026 8:20:02 PM.456] UNHANDLED EXCEPTION DURING GAME FIBER TICK
[10/23/2026 8:20:02 PM.456] ------------------------------
[10/23/2026 8:20:02 PM.456] Origin: Game fiber "<UNNAMED THREAD>".
[10/23/2026 8:20:02 PM.456] ------------------------------
[10/23/2026 8:20:02 PM.456] Exception type: System.NullReferenceException
[10/23/2026 8:20:02 PM.456] Exception message: Object reference not set to an instance of an object.
[10/23/2026 8:20:02 PM.456] ------------------------------
[10/23/2026 8:20:02 PM.456] Stack trace:
[10/23/2026 8:20:02 PM.456] at Rage.Ped.get_Position()
[10/23/2026 8:20:02 PM.456] at UltimateBackup.API.Functions.Go(Ped ped) in C:\src\Functions.cs:line 12
[10/23/2026 8:20:02 PM.456] at Rage.GameFiber.Main()
[10/23/2026 8:20:02 PM.456] ==============================
[10/23/2026 8:21:00 PM.000] Exception type: System.IO.FileNotFoundException
[10/23/2026 8:21:00 PM.000] Exception message: Could not load file or assembly 'RAGENativeUI, Version=1.9.2.0, Culture=neutral, PublicKeyToken=null' or one of its dependencies.
[10/23/2026 8:21:00 PM.000] at CompuLite.EntryPoint.Main()
[10/23/2026 8:22:00 PM.000] Plugin "BetterEMS" was terminated because it caused an unhandled exception
"#;

    #[test]
    fn finds_who_crashed() {
        let found = check(LOG);
        assert_eq!(found.len(), 3, "{found:#?}");
        assert_eq!(found[0].culprit.as_deref(), Some("UltimateBackup"));
        assert!(found[0].detail.contains("NullReferenceException"));
        assert_eq!(found[1].missing.as_deref(), Some("RAGENativeUI"));
        assert_eq!(found[1].culprit.as_deref(), Some("CompuLite"));
        assert_eq!(found[2].culprit.as_deref(), Some("BetterEMS"));
        assert!(check("[10/23/2026 8:15:31 PM.123] all fine\n").is_empty());
    }

    #[test]
    fn blames_the_installed_mod() {
        let m = |id: u64, name: &str, dll: &str| Mod {
            id, name: name.into(), source: String::new(), installed: 0, kinds: vec![], game: PathBuf::new(), enabled: true, profiles: None, models: vec![],
            files: vec![crate::install::FileRec { path: PathBuf::from(dll), backup: None }],
        };
        let mods = [m(1, "Ultimate Backup 1.8", r"plugins\LSPDFR\Ultimate Backup.dll"), m(2, "Stop The Ped", r"plugins\LSPDFR\StopThePed.dll")];
        let f = Finding { title: String::new(), detail: String::new(), culprit: Some("UltimateBackup".into()), missing: None };
        assert_eq!(blame(&f, &mods).map(|m| m.id), Some(1));
    }
}
