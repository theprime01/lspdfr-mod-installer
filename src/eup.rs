//! Emergency Uniforms Pack: the Law & Order base pack (an LML package),
//! EUP Menu, and add-on packs that retexture or add EUP clothing.
//!
//! Add-on packs become LML packages that load after EUP. Their saved
//! outfits (a wardrobe.ini) become their own agency in EUP Menu, kept in
//! `plugins\EUP\packs\` and copied into presetoutfits.ini while the pack
//! is on.

use std::fs;
use std::path::{Path, PathBuf};

/// Where add-on packs' EUP Menu outfits wait, one file per pack.
pub const PACKS: &str = r"plugins\EUP\packs";
pub const PRESETS: &str = r"plugins\EUP\presetoutfits.ini";
pub const SETTINGS: &str = r"plugins\EUP\settings.ini";
/// The EUP version add-on packs are made for now.
pub const CURRENT: (u32, u32) = (8, 3);

/// A ped clothing set folder, named the way the game names them:
/// `mp_m_freemode_01`, `mp_f_freemode_01_mp_f_heist4`, `mp_m_freemode_01_p` …
pub fn is_collection(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("mp_m_freemode_01") || n.starts_with("mp_f_freemode_01")
}

/// An install.xml from the EUP team (Law & Order or Serve & Rescue).
pub fn is_eup_package(install_xml: &str) -> bool {
    let x = install_xml.to_lowercase();
    x.contains("<mountname>eup") || x.contains("emergency-uniforms-pack") || x.contains("<name>eup")
}

/// Folders an LML package lays over the game folder (`<GameFolder>`).
pub fn game_folders(install_xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = install_xml;
    while let Some(i) = rest.find("<GameFolder>") {
        rest = &rest[i + "<GameFolder>".len()..];
        let path = rest.split('<').next().unwrap_or("").trim().replace('\\', "/");
        if !path.is_empty() {
            out.push(path.trim_matches('/').to_lowercase());
        }
    }
    out
}

/// The EUP Menu agency for a pack, from its name: "Met Police EUP 1.1 New
/// Install" is "Met Police".
pub fn agency_name(mod_name: &str) -> String {
    const NOISE: &[&str] = &["eup", "pack", "new", "install", "installer", "update", "lml", "oiv", "uniforms", "uniform", "-", "_", "[eup]", "(eup)"];
    let words: Vec<&str> = mod_name
        .split_whitespace()
        .filter(|w| {
            let l = w.to_lowercase();
            !NOISE.contains(&l.as_str()) && !l.trim_start_matches('v').chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .collect();
    if words.is_empty() { mod_name.trim().to_string() } else { words.join(" ") }
}

/// A wardrobe.ini as EUP Menu preset outfits under `agency`, with each
/// outfit's first word ("FP", "TSG") as its division when there are several.
pub fn presets_from_wardrobe(agency: &str, wardrobe: &str) -> String {
    let mut outfits: Vec<(String, Vec<String>)> = Vec::new();
    for line in wardrobe.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            outfits.push((name.trim().to_string(), Vec::new()));
        } else if let Some((_, body)) = outfits.last_mut().filter(|_| line.contains('=') && !line.starts_with(';') && !line.starts_with("//")) {
            body.push(line.to_string());
        }
    }
    // A unit code ("LPU", "TSG") on its own, or else the first two words
    // ("Public Order Soft" is in "Public Order").
    let division = |name: &str| {
        let words: Vec<&str> = name.split_whitespace().collect();
        match words.as_slice() {
            [first, ..] if first.len() <= 5 && first.chars().all(|c| c.is_ascii_uppercase()) => first.to_string(),
            [first, second, _, ..] => format!("{first} {second}"),
            [first, ..] => first.to_string(),
            [] => String::new(),
        }
    };
    let mut divisions: Vec<String> = outfits.iter().map(|(n, _)| division(n)).collect();
    divisions.dedup();
    let split = divisions.len() > 1;
    let mut out = String::new();
    for (name, body) in &outfits {
        let gender = body.iter().find_map(|l| l.strip_prefix("Gender=")).unwrap_or("Male").trim().to_string();
        out += &format!("[{gender} {agency} {name}]\r\nGender={gender}\r\nCategory=EUP\r\nCategory2={agency}\r\n");
        if split {
            out += &format!("Category3={}\r\n", division(name));
        }
        for l in body.iter().filter(|l| !l.starts_with("Gender=") && !l.starts_with("Category")) {
            out += &format!("{l}\r\n");
        }
        out += "\r\n";
    }
    out
}

/// True for an EUP Menu wardrobe or preset file: outfits with clothing IDs.
pub fn is_wardrobe(text: &str) -> bool {
    text.contains("Gender=") && text.contains("Top=") && text.contains('[')
}

/// The EUP version a readme says a pack was made for, if it's older than
/// the current one: "Install EUP 8.1" is (8, 1).
pub fn made_for_old_eup(readme: &str) -> Option<(u32, u32)> {
    let l = readme.to_lowercase();
    let mut best: Option<(u32, u32)> = None;
    for key in ["eup ", "law & order ", "law and order ", "law&order "] {
        let mut rest = l.as_str();
        while let Some(i) = rest.find(key) {
            rest = &rest[i + key.len()..];
            let v: String = rest.trim_start_matches(['v', ' ']).chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let mut parts = v.split('.').filter_map(|p| p.parse::<u32>().ok());
            if let (Some(major), Some(minor)) = (parts.next(), parts.next()) {
                if (5..=20).contains(&major) {
                    best = Some(best.map_or((major, minor), |b| b.max((major, minor))));
                }
            }
        }
    }
    best.filter(|v| *v < CURRENT)
}

/// The block markers of a pack's outfits inside presetoutfits.ini.
fn markers(pack: &str) -> (String, String) {
    (format!("// >>> LSPDFR Mod Installer: {pack}"), format!("// <<< LSPDFR Mod Installer: {pack}"))
}

/// presetoutfits.ini with exactly these packs' outfits in their blocks,
/// and everything else left as it was.
pub fn presets_with(text: &str, packs: &[(String, String)]) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("// >>> LSPDFR Mod Installer: ") {
            inside = true;
        } else if line.starts_with("// <<< LSPDFR Mod Installer: ") {
            inside = false;
        } else if !inside {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut out: String = kept.iter().map(|l| format!("{l}\r\n")).collect();
    for (pack, outfits) in packs {
        let (start, end) = markers(pack);
        out += &format!("\r\n{start}\r\n{}\r\n{end}\r\n", outfits.trim_end());
    }
    out
}

pub struct Status {
    /// The Law & Order package folder, or the dlcpacks folder.
    pub law_order: Option<PathBuf>,
    pub law_order_version: Option<String>,
    pub serve_rescue: bool,
    pub menu_version: Option<[u16; 4]>,
    /// The key that opens EUP Menu, from its settings.ini.
    pub menu_key: Option<String>,
}

pub fn status(game: &Path) -> Status {
    let mut s = Status { law_order: None, law_order_version: None, serve_rescue: false, menu_version: None, menu_key: None };
    for dir in fs::read_dir(game.join("lml")).into_iter().flatten().flatten().map(|e| e.path()) {
        let Ok(xml) = fs::read_to_string(dir.join("install.xml")) else { continue };
        let l = xml.to_lowercase();
        if l.contains("law &amp; order") || l.contains("law & order") {
            s.law_order_version = tag(&xml, "Version");
            s.law_order = Some(dir);
        } else if l.contains("serve &amp; rescue") || l.contains("serve & rescue") {
            s.serve_rescue = true;
        }
    }
    let dlc = game.join(r"mods\update\x64\dlcpacks");
    if s.law_order.is_none() && dlc.join("eup").is_dir() {
        s.law_order = Some(dlc.join("eup"));
    }
    s.serve_rescue |= dlc.join("eup_sr").is_dir() || dlc.join("eupsr").is_dir();
    s.menu_version = crate::dotnet::file_version(&game.join(r"plugins\EUPMenu.dll"));
    s.menu_key = fs::read_to_string(game.join(SETTINGS)).ok().and_then(|t| {
        let key = t.lines().find_map(|l| l.trim().strip_prefix("MenuToggle="))?.trim().to_string();
        let modifier = t.lines().find_map(|l| l.trim().strip_prefix("MenuToggleModifier=")).map(str::trim).filter(|m| !m.eq_ignore_ascii_case("none"));
        Some(match modifier {
            Some(m) => format!("{m} + {key}"),
            None => key,
        })
    });
    s
}

/// The text of the first `<name>` element.
pub fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let i = xml.find(&open)? + open.len();
    let v = xml[i..].split('<').next()?.trim();
    (!v.is_empty()).then(|| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_agencies_from_pack_names() {
        assert_eq!(agency_name("Met Police EUP 1.1 New Install"), "Met Police");
        assert_eq!(agency_name("[EUP] Police Scotland v2.0"), "Police Scotland");
        assert_eq!(agency_name("EUP"), "EUP");
    }

    #[test]
    fn turns_a_wardrobe_into_an_agency() {
        let w = "[FP Shirt A]\r\nGender=Male\r\nTop=201:1\r\n\r\n[TSG Soft]\r\nGender=Female\r\nHat=127:1\r\n";
        let p = presets_from_wardrobe("Met Police", w);
        assert_eq!(p, "[Male Met Police FP Shirt A]\r\nGender=Male\r\nCategory=EUP\r\nCategory2=Met Police\r\nCategory3=FP\r\nTop=201:1\r\n\r\n[Female Met Police TSG Soft]\r\nGender=Female\r\nCategory=EUP\r\nCategory2=Met Police\r\nCategory3=TSG\r\nHat=127:1\r\n\r\n");
        assert!(is_wardrobe(w));
        let santos = presets_from_wardrobe("Santos Constabulary", "[LPU Coat Shirt]\r\nGender=Male\r\n[Public Order Soft]\r\nGender=Male\r\n");
        assert!(santos.contains("Category3=LPU\r\n") && santos.contains("Category3=Public Order\r\n"));
    }

    #[test]
    fn spots_packs_made_for_older_eup() {
        assert_eq!(made_for_old_eup("==> Install EUP 8.1\n==> Using OpenIV"), Some((8, 1)));
        assert_eq!(made_for_old_eup("Requires EUP Law & Order 8.3"), None);
        assert_eq!(made_for_old_eup("Works with EUP 7.4 and EUP 8.3"), None);
        assert_eq!(made_for_old_eup("no versions here, eup menu"), None);
    }

    #[test]
    fn pack_blocks_come_and_go_without_touching_the_rest() {
        let base = "// header\r\n[Male LSPD Class A]\r\nGender=Male\r\n";
        let one = presets_with(base, &[("Met".into(), "[Male Met A]\r\nGender=Male\r\n".into())]);
        assert!(one.starts_with(base));
        assert!(one.contains("// >>> LSPDFR Mod Installer: Met\r\n[Male Met A]"));
        assert_eq!(presets_with(&one, &[("Met".into(), "[Male Met A]\r\nGender=Male\r\n".into())]), one);
        assert_eq!(presets_with(&one, &[]), base);
    }

    #[test]
    fn reads_game_folder_overlays() {
        assert_eq!(game_folders("<ResourceFolders>\n<GameFolder>content/configs</GameFolder>\n</ResourceFolders>"), ["content/configs"]);
        assert!(is_eup_package("<Name>EUP - Law &amp; Order Public</Name><MountName>eup</MountName>"));
        assert!(is_collection("mp_m_freemode_01_p_mp_m_christmas2017"));
        assert!(!is_collection("stream"));
    }
}
