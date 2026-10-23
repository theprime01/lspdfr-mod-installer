//! Finds GTA V and checks which of the LSPDFR essentials are installed.

use std::path::{Path, PathBuf};

use crate::analyze::Kind;

pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|p| is_game(p))
}

pub fn is_game(dir: &Path) -> bool {
    dir.join("GTA5.exe").is_file() || dir.join("PlayGTAV.exe").is_file()
}

/// GTA V Enhanced, which LSPDFR and RAGE Plugin Hook don't support.
pub fn is_enhanced(dir: &Path) -> bool {
    dir.join("GTA5_Enhanced.exe").is_file() && !dir.join("GTA5.exe").is_file()
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in [r"SOFTWARE\WOW6432Node\Rockstar Games\Grand Theft Auto V", r"SOFTWARE\Rockstar Games\Grand Theft Auto V"] {
        if let Some(dir) = registry_string(key, "InstallFolder") {
            out.push(PathBuf::from(dir));
        }
    }
    let steam = registry_string(r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam"));
    let mut libraries = vec![steam.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        libraries.extend(steam_libraries(&vdf));
    }
    for lib in libraries {
        out.push(lib.join("steamapps").join("common").join("Grand Theft Auto V"));
        out.push(lib.join("steamapps").join("common").join("Grand Theft Auto V Legacy"));
    }
    if let Ok(entries) = std::fs::read_dir(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests") {
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            if json["DisplayName"].as_str().is_some_and(|n| n.contains("Grand Theft Auto V")) {
                if let Some(dir) = json["InstallLocation"].as_str() {
                    out.push(PathBuf::from(dir));
                }
            }
        }
    }
    out
}

/// `"path"` values from Steam's libraryfolders.vdf.
pub fn steam_libraries(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?;
            let value = rest.trim().trim_matches('"');
            Some(PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

fn registry_string(key: &str, value: &str) -> Option<String> {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::HSTRING;
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if r.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The game build, e.g. 3889, from GTA5.exe's version (1.0.3889.0).
pub fn build(dir: &Path) -> Option<u32> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW};
    use windows::core::{HSTRING, w};
    let exe = HSTRING::from(dir.join("GTA5.exe").as_os_str());
    unsafe {
        let size = GetFileVersionInfoSizeW(&exe, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&exe, 0, size, data.as_mut_ptr().cast()).ok()?;
        let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut info, &mut len).as_bool() || info.is_null() {
            return None;
        }
        let info = &*(info as *const VS_FIXEDFILEINFO);
        Some(info.dwFileVersionLS >> 16)
    }
}

/// True while GTA V is running, when its files are locked.
pub fn running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if matches!(name.as_str(), "gta5.exe" | "ragepluginhook.exe" | "gta5_enhanced.exe") {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    ScriptHookV,
    AsiLoader,
    Shvdn,
    Rph,
    Lspdfr,
    RageNativeUi,
    Lml,
    OpenIvAsi,
    Els,
}

pub struct Prereq {
    pub need: Need,
    pub name: &'static str,
    pub what: &'static str,
    pub url: &'static str,
    pub essential: bool,
}

pub const PREREQS: &[Prereq] = &[
    Prereq { need: Need::Rph, name: "RAGE Plugin Hook", what: "Runs LSPDFR and its plugins. Comes with LSPDFR.", url: "https://ragepluginhook.net/", essential: true },
    Prereq { need: Need::Lspdfr, name: "LSPDFR", what: "The police mod itself.", url: "https://www.lcpdfr.com/downloads/gta5mods/g17media/7792-lspd-first-response/", essential: true },
    Prereq { need: Need::RageNativeUi, name: "RAGENativeUI", what: "Menus used by most LSPDFR plugins.", url: "https://github.com/alexguirre/RAGENativeUI/releases", essential: true },
    Prereq { need: Need::Lml, name: "Lenny's Mod Loader", what: "Loads cars and other add-ons without editing game archives.", url: "https://www.lcpdfr.com/downloads/gta5mods/misc/25437-lennys-mod-loader/", essential: true },
    Prereq { need: Need::ScriptHookV, name: "Script Hook V", what: "Needed for .asi mods and .NET scripts.", url: "http://www.dev-c.com/gtav/scripthookv/", essential: false },
    Prereq { need: Need::AsiLoader, name: "ASI Loader (dinput8.dll)", what: "Loads .asi mods. Comes with Script Hook V.", url: "http://www.dev-c.com/gtav/scripthookv/", essential: false },
    Prereq { need: Need::Shvdn, name: "Script Hook V .NET", what: "Runs scripts in the scripts folder.", url: "https://github.com/scripthookvdotnet/scripthookvdotnet/releases", essential: false },
    Prereq { need: Need::OpenIvAsi, name: "OpenIV.asi", what: "Lets the game read unencrypted DLC packs and the mods folder.", url: "https://openiv.com/", essential: false },
    Prereq { need: Need::Els, name: "Emergency Lighting System", what: "Needed by cars that come with an ELS .xml.", url: "https://www.lcpdfr.com/search/?q=Emergency%20Lighting%20System&type=downloads_file", essential: false },
];

pub fn has(game: &Path, need: Need) -> bool {
    let f = |p: &str| game.join(p).exists();
    match need {
        Need::ScriptHookV => f("ScriptHookV.dll"),
        Need::AsiLoader => f("dinput8.dll") || f("version.dll") || f("xinput1_4.dll"),
        Need::Shvdn => f("ScriptHookVDotNet.asi"),
        Need::Rph => f("RAGEPluginHook.exe"),
        Need::Lspdfr => f(r"plugins\LSPD First Response.dll"),
        Need::RageNativeUi => f("RAGENativeUI.dll"),
        Need::Lml => f("vfs.dll") || f("vfs.asi"),
        Need::OpenIvAsi => f("OpenIV.asi"),
        Need::Els => f("ELS.asi"),
    }
}

/// What a kind of mod needs in order to work.
pub fn needs(kind: Kind) -> &'static [Need] {
    match kind {
        Kind::LspdfrPlugin | Kind::LspdfrFiles => &[Need::Rph, Need::Lspdfr],
        Kind::RphPlugin => &[Need::Rph],
        Kind::Script => &[Need::ScriptHookV, Need::Shvdn],
        Kind::Asi => &[Need::ScriptHookV, Need::AsiLoader],
        Kind::AddonVehicle | Kind::ReplaceVehicle | Kind::LmlMod => &[Need::Lml],
        Kind::DlcPack => &[Need::Lml, Need::OpenIvAsi],
        Kind::Els => &[Need::Els],
        Kind::Library | Kind::GameFiles => &[],
    }
}

pub fn prereq(need: Need) -> &'static Prereq {
    PREREQS.iter().find(|p| p.need == need).expect("every need is listed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_steam_library_paths() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"G:\\\\SteamLibrary\"\n\t}\n}";
        assert_eq!(steam_libraries(vdf), [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"G:\SteamLibrary")]);
    }

    #[test]
    #[ignore]
    fn finds_this_pc_s_game() {
        let game = find().expect("GTA V");
        println!("{} build {:?}", game.display(), build(&game));
        assert!(build(&game).is_some());
    }
}
