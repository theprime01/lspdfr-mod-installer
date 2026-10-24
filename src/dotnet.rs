//! Works out what a .dll is from the names in its .NET metadata: an LSPDFR
//! plugin, a RAGE Plugin Hook plugin, a Script Hook V .NET script, or a
//! library that belongs in the game folder.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dll {
    LspdfrPlugin,
    RphPlugin,
    ShvdnScript,
    Library,
}

/// Libraries that reference RPH or LSPDFR but live in the game folder.
const ROOT_LIBRARIES: &[&str] = &[
    "ragenativeui.dll",
    "albo1125.common.dll",
    "calloutinterfaceapi.dll",
    "newtonsoft.json.dll",
];

pub fn classify(path: &Path) -> Dll {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name == "lspd first response.dll" {
        return Dll::RphPlugin;
    }
    if ROOT_LIBRARIES.contains(&name.as_str()) || name.starts_with("naudio") || name.starts_with("system.") || name.starts_with("microsoft.") {
        return Dll::Library;
    }
    match std::fs::read(path) {
        Ok(bytes) => classify_bytes(&bytes),
        Err(_) => Dll::Library,
    }
}

pub fn classify_bytes(bytes: &[u8]) -> Dll {
    if !is_dotnet(bytes) {
        return Dll::Library;
    }
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    // Type names sit null-terminated in the metadata string heap.
    if has(b"LSPD First Response") && has(b"\0Plugin\0") {
        Dll::LspdfrPlugin
    } else if has(b"RagePluginHook") && has(b"PluginAttribute") {
        Dll::RphPlugin
    } else if has(b"ScriptHookVDotNet") && has(b"\0Script\0") {
        Dll::ShvdnScript
    } else {
        Dll::Library
    }
}

/// The file version from a .dll or .asi's version resource, if it has one.
pub fn file_version(path: &Path) -> Option<[u16; 4]> {
    file_version_bytes(&std::fs::read(path).ok()?)
}

/// Finds VS_FIXEDFILEINFO by its signature; the version follows the
/// signature and structure version.
pub fn file_version_bytes(b: &[u8]) -> Option<[u16; 4]> {
    let sig = 0xFEEF04BDu32.to_le_bytes();
    let at = b.windows(4).position(|w| w == sig)?;
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let (ms, ls) = (u32_at(at + 8)?, u32_at(at + 12)?);
    Some([(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16])
}

/// True for a PE file with a CLR header.
fn is_dotnet(b: &[u8]) -> bool {
    let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(0..2) != Some(b"MZ") {
        return false;
    }
    let Some(pe) = u32_at(0x3c).map(|v| v as usize) else { return false };
    if b.get(pe..pe + 4) != Some(b"PE\0\0") {
        return false;
    }
    let opt = pe + 24;
    let dirs = match u16_at(opt) {
        Some(0x10b) => opt + 96,
        Some(0x20b) => opt + 112,
        _ => return false,
    };
    // Data directory 14 is the CLR runtime header.
    u32_at(dirs + 14 * 8).is_some_and(|rva| rva != 0)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A minimal PE32 image with a CLR header and the given metadata strings.
    pub fn fake_dll(strings: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let opt = 0x80 + 24;
        b[opt..opt + 2].copy_from_slice(&0x10bu16.to_le_bytes());
        let clr = opt + 96 + 14 * 8;
        b[clr..clr + 4].copy_from_slice(&0x2008u32.to_le_bytes());
        for s in strings {
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        b
    }

    /// A version resource's fixed part with this version.
    pub fn versioned(v: [u16; 4]) -> Vec<u8> {
        let mut b = b"MZ padding ".to_vec();
        b.extend_from_slice(&0xFEEF04BDu32.to_le_bytes());
        b.extend_from_slice(&0x10000u32.to_le_bytes());
        b.extend_from_slice(&(((v[0] as u32) << 16) | v[1] as u32).to_le_bytes());
        b.extend_from_slice(&(((v[2] as u32) << 16) | v[3] as u32).to_le_bytes());
        b
    }

    #[test]
    #[ignore]
    fn reads_this_pc_s_ragenativeui_version() {
        let game = crate::game::find().expect("GTA V");
        println!("{:?}", file_version(&game.join("RAGENativeUI.dll")));
        assert!(file_version(&game.join("RAGENativeUI.dll")).is_some());
    }

    #[test]
    fn reads_file_versions() {
        assert_eq!(file_version_bytes(&versioned([1, 9, 3, 0])), Some([1, 9, 3, 0]));
        assert_eq!(file_version_bytes(b"MZ no version"), None);
    }

    #[test]
    fn sorts_dlls_by_what_they_reference() {
        assert_eq!(classify_bytes(&fake_dll(&["", "LSPD First Response", "Plugin", "Callout"])), Dll::LspdfrPlugin);
        assert_eq!(classify_bytes(&fake_dll(&["", "RagePluginHook", "PluginAttribute"])), Dll::RphPlugin);
        assert_eq!(classify_bytes(&fake_dll(&["", "ScriptHookVDotNet3", "Script"])), Dll::ShvdnScript);
        // References LSPDFR but isn't a plugin: an API library.
        assert_eq!(classify_bytes(&fake_dll(&["", "LSPD First Response", "Functions"])), Dll::Library);
        assert_eq!(classify_bytes(b"MZ not really a dll"), Dll::Library);
    }
}
