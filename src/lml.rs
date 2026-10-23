//! Turns vehicle (and other streamed) mods into Lenny's Mod Loader packages:
//! a folder in `lml\` with the files and an install.xml describing them.

use std::path::{Path, PathBuf};

/// File types the game streams (models, textures, collisions, maps).
pub const STREAMED: &[&str] = &["yft", "ytd", "ydr", "ydd", "ycd", "ybn", "ymap", "ytyp", "ynv", "ypt", "ynd", "yld", "ymf"];

pub fn is_streamed(path: &Path) -> bool {
    STREAMED.contains(&crate::extract::ext(path).as_str())
}

/// The data file type for a meta file, from its name.
pub fn data_type(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let t = if n.ends_with("vehicles.meta") {
        "VEHICLE_METADATA_FILE"
    } else if n.ends_with("handling.meta") {
        "HANDLING_FILE"
    } else if n.ends_with("carvariations.meta") {
        "VEHICLE_VARIATION_FILE"
    } else if n.ends_with("carcols.meta") || n.ends_with("carcols.ymt") {
        "CARCOLS_FILE"
    } else if n.contains("layouts") && n.ends_with(".meta") {
        "VEHICLE_LAYOUTS_FILE"
    } else if n.ends_with("dlctext.meta") {
        "DLC_TEXT_FILE"
    } else if n.ends_with("contentunlocks.meta") {
        "CONTENT_UNLOCKING_META_FILE"
    } else if n.ends_with("peds.meta") || n.ends_with("peds.ymt") {
        "PED_METADATA_FILE"
    } else if n.starts_with("weapon") && n.ends_with(".meta") && n.contains("archetype") {
        "WEAPON_METADATA_FILE"
    } else if n.starts_with("weapon") && n.ends_with(".meta") {
        "WEAPONINFO_FILE"
    } else if n.ends_with("shop_vehicle.meta") {
        "VEHICLE_SHOP_DLC_FILE"
    } else {
        return None;
    };
    Some(t)
}

/// `data_file 'TYPE' 'path'` lines from a FiveM fxmanifest.lua or
/// __resource.lua, as (type, path) with `/` separators.
pub fn lua_data_files(lua: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in lua.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("data_file") else { continue };
        let quoted: Vec<&str> = rest.split(['\'', '"']).enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
        if let [kind, path, ..] = quoted.as_slice() {
            let mut path = path.replace('\\', "/");
            while path.contains("//") {
                path = path.replace("//", "/");
            }
            out.push((kind.to_string(), path.trim_start_matches("./").to_string()));
        }
    }
    out
}

pub struct DataFile {
    pub kind: Option<String>,
    /// Path inside the package, with `/` separators.
    pub path: String,
}

pub enum Package {
    /// New content: streamed files in `stream/`, meta files in `data/`.
    Addon { streaming: bool, data: Vec<DataFile> },
    /// A FiveM resource left as it is and loaded from its lua file.
    Resource { lua: String },
    /// Files that replace the game's own, in `replace/`.
    Replace,
    /// A packed DLC.
    DlcRpf,
}

pub fn install_xml(name: &str, package: &Package) -> String {
    let name = escape(name);
    let body = match package {
        Package::Addon { streaming, data } => {
            let mut s = format!("    <Addons>\n        <Addon name=\"{name}\">\n");
            if *streaming {
                s += "            <StreamingFiles>stream</StreamingFiles>\n";
            }
            for d in data {
                match &d.kind {
                    Some(kind) => s += &format!("            <DataFile type=\"{}\">{}</DataFile>\n", escape(kind), escape(&d.path)),
                    None => s += &format!("            <DataFile>{}</DataFile>\n", escape(&d.path)),
                }
            }
            s + "        </Addon>\n    </Addons>\n"
        }
        Package::Resource { lua } => {
            format!("    <Addons>\n        <Addon name=\"{name}\">\n            <ResourceLua>{}</ResourceLua>\n        </Addon>\n    </Addons>\n", escape(lua))
        }
        Package::Replace => {
            format!("    <Replacements>\n        <Replacement name=\"{name}\">\n            <FileReplacements>replace</FileReplacements>\n        </Replacement>\n    </Replacements>\n")
        }
        Package::DlcRpf => {
            format!("    <Addons>\n        <Addon name=\"{name}\">\n            <DlcRpf>dlc.rpf</DlcRpf>\n        </Addon>\n    </Addons>\n")
        }
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<EasyInstall>\n    <Name>{name}</Name>\n    <Author>Installed by LSPDFR Mod Installer</Author>\n    <Version>1.0</Version>\n    <Link></Link>\n    <Metadata>\n    </Metadata>\n{body}</EasyInstall>\n"
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A folder name that is safe in `lml\`.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() { "mod".to_string() } else { cleaned }
}

pub fn root(name: &str) -> PathBuf {
    PathBuf::from("lml").join(folder_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_data_files_from_fivem_manifests() {
        let lua = "fx_version 'cerulean'\nfiles { 'data/**/*.meta' }\ndata_file 'HANDLING_FILE' 'data/handling.meta'\ndata_file \"VEHICLE_METADATA_FILE\" \"data\\\\vehicles.meta\"\n-- data_file\n";
        let found = lua_data_files(lua);
        assert_eq!(found, vec![
            ("HANDLING_FILE".to_string(), "data/handling.meta".to_string()),
            ("VEHICLE_METADATA_FILE".to_string(), "data/vehicles.meta".to_string()),
        ]);
    }

    #[test]
    fn writes_an_addon_install_xml() {
        let xml = install_xml("Police & Co", &Package::Addon {
            streaming: true,
            data: vec![DataFile { kind: Some("VEHICLE_METADATA_FILE".into()), path: "data/vehicles.meta".into() }],
        });
        assert!(xml.contains("<Name>Police &amp; Co</Name>"));
        assert!(xml.contains("<StreamingFiles>stream</StreamingFiles>"));
        assert!(xml.contains("<DataFile type=\"VEHICLE_METADATA_FILE\">data/vehicles.meta</DataFile>"));
    }

    #[test]
    fn knows_common_meta_files() {
        assert_eq!(data_type("vehicles.meta"), Some("VEHICLE_METADATA_FILE"));
        assert_eq!(data_type("POLICE_handling.meta"), Some("HANDLING_FILE"));
        assert_eq!(data_type("vehiclelayouts.meta"), Some("VEHICLE_LAYOUTS_FILE"));
        assert_eq!(data_type("readme.txt"), None);
        assert_eq!(folder_name("2020 Ford: Explorer?"), "2020 Ford_ Explorer_");
    }
}
