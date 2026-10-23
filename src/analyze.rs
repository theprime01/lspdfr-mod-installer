//! Looks at an unpacked mod and works out where every file goes.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dotnet::{self, Dll};
use crate::extract::ext;
use crate::lml::{self, DataFile, Package};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    LspdfrPlugin,
    RphPlugin,
    Script,
    Asi,
    AddonVehicle,
    ReplaceVehicle,
    DlcPack,
    LmlMod,
    Els,
    LspdfrFiles,
    Library,
    GameFiles,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::LspdfrPlugin => "LSPDFR plugin",
            Kind::RphPlugin => "RAGE Plugin Hook plugin",
            Kind::Script => "Script Hook V .NET script",
            Kind::Asi => "ASI mod",
            Kind::AddonVehicle => "Add-on vehicle",
            Kind::ReplaceVehicle => "Replacement vehicle",
            Kind::DlcPack => "DLC pack",
            Kind::LmlMod => "Mod Loader package",
            Kind::Els => "ELS lighting config",
            Kind::LspdfrFiles => "LSPDFR files",
            Kind::Library => "Library",
            Kind::GameFiles => "Game folder files",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Copy { from: PathBuf, to: PathBuf },
    Write { to: PathBuf, contents: String },
}

impl Action {
    /// Destination, relative to the game folder.
    pub fn to(&self) -> &Path {
        match self {
            Action::Copy { to, .. } | Action::Write { to, .. } => to,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub kind: Kind,
    pub label: String,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: &'static str,
}

#[derive(Clone, Debug)]
pub struct Set {
    pub label: String,
    pub parts: Vec<Part>,
    pub skipped: Vec<Skipped>,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub name: String,
    pub sets: Vec<Set>,
    /// The sets are alternative versions of the same mod: install one.
    pub exclusive: bool,
}

impl Plan {
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.sets.iter().filter(|s| s.selected).flat_map(|s| s.parts.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.sets.iter().all(|s| s.parts.is_empty())
    }

    /// True when the user has to choose before installing.
    pub fn needs_decision(&self) -> bool {
        self.is_empty() || (self.exclusive && self.sets.len() > 1)
    }

    pub fn kinds(&self) -> Vec<Kind> {
        let mut k: Vec<Kind> = self.parts().map(|p| p.kind).collect();
        k.sort();
        k.dedup();
        k
    }

    pub fn select(&mut self, index: usize, on: bool) {
        if self.exclusive && on {
            for s in &mut self.sets {
                s.selected = false;
            }
        }
        if let Some(s) = self.sets.get_mut(index) {
            s.selected = on;
        }
    }
}

const GAME_FOLDERS: &[&str] = &["plugins", "scripts", "lspdfr", "els", "lml", "mods"];
const GAME_ROOT_FILES: &[&str] = &["scripthookv.dll", "dinput8.dll", "ragepluginhook.exe", "vfs.dll", "scripthookvdotnet.asi"];
/// Folder names that say nothing about the mod itself.
const GENERIC: &[&str] = &[
    "stream", "data", "addon", "add-on", "addons", "replace", "replacement", "els", "non-els", "non els", "nonels", "files", "resource",
    "fivem", "singleplayer", "single player", "sp", "grand theft auto v", "gta v", "gtav", "gta5", "main files", "install", "vehicle", "vehicles",
];
const OPTIONAL: &[&str] = &["optional", "extra", "extras", "alternative", "alternate", "old", "legacy", "backup", "original", "uninstall"];
const DOCS: &[&str] = &["txt", "md", "pdf", "url", "html", "htm", "rtf", "doc", "docx", "nfo", "png", "jpg", "jpeg", "gif", "bmp", "webp", "mp4", "psd"];

pub fn analyze(root: &Path, name: &str) -> Plan {
    let top = unwrap(root);
    let groups: Vec<PathBuf> = subdirs(&top).into_iter().filter(|d| has_content(d)).collect();
    let loose = files(&top).iter().any(|f| !is_doc(f, true) && is_installable(f));
    let mut ctx = Ctx { base: root.to_path_buf(), name: name.to_string(), lml_used: HashSet::new() };
    let sets: Vec<Set> = if is_mirror(&top) || loose || groups.len() <= 1 {
        vec![ctx.set(&top, name.to_string(), true)]
    } else {
        groups.iter().map(|g| {
            let label = file_name(g);
            let optional = OPTIONAL.iter().any(|o| label.to_lowercase().contains(o));
            ctx.set(g, label, !optional)
        }).collect()
    };
    let mut plan = Plan { name: name.to_string(), exclusive: collide(&sets), sets };
    if plan.exclusive {
        let first = plan.sets.iter().position(|s| s.selected).unwrap_or(0);
        plan.select(first, true);
    }
    plan
}

struct Ctx {
    base: PathBuf,
    name: String,
    lml_used: HashSet<String>,
}

impl Ctx {
    fn set(&mut self, dir: &Path, label: String, selected: bool) -> Set {
        let mut out = Out::default();
        self.walk(dir, &mut out);
        out.merge_parts();
        Set { label, parts: out.parts, skipped: out.skipped, selected }
    }

    fn rel(&self, p: &Path) -> PathBuf {
        p.strip_prefix(&self.base).map(Path::to_path_buf).unwrap_or_else(|_| p.to_path_buf())
    }

    fn skip(&self, out: &mut Out, p: &Path, reason: &'static str) {
        out.skipped.push(Skipped { path: self.rel(p), reason });
    }

    /// A unique `lml\` folder name, from the folder unless it is generic.
    fn lml_name(&mut self, dir: &Path) -> String {
        let own = file_name(dir);
        let base = if GENERIC.contains(&own.to_lowercase().as_str()) || own.is_empty() { self.name.clone() } else { own };
        let base = lml::folder_name(&base);
        let mut name = base.clone();
        let mut n = 2;
        while !self.lml_used.insert(name.to_lowercase()) {
            name = format!("{base} ({n})");
            n += 1;
        }
        name
    }

    fn walk(&mut self, dir: &Path, out: &mut Out) {
        if is_mirror(dir) {
            return self.mirror(dir, out);
        }
        if vehicle_root(dir) {
            return self.vehicle(dir, out);
        }
        let files = files(dir);
        let top = dir == self.base || unwrap(&self.base) == dir;

        // Plugins first: their folder is where the files around them go.
        let mut home: Option<(usize, PathBuf)> = None;
        let mut rank = 99;
        for f in files.iter().filter(|f| ext(f) == "dll") {
            let (kind, dest, r) = match dotnet::classify(f) {
                Dll::LspdfrPlugin => (Kind::LspdfrPlugin, PathBuf::from("plugins").join("LSPDFR"), 0),
                Dll::RphPlugin => (Kind::RphPlugin, PathBuf::from("plugins"), 1),
                Dll::ShvdnScript => (Kind::Script, PathBuf::from("scripts"), 2),
                Dll::Library => (Kind::Library, PathBuf::new(), 4),
            };
            let index = out.add(kind, stem(f), Action::Copy { from: f.clone(), to: dest.join(file_name(f)) });
            if r < rank {
                rank = r;
                home = Some((index, dest));
            }
        }
        for f in files.iter().filter(|f| ext(f) == "asi") {
            let index = out.add(Kind::Asi, stem(f), Action::Copy { from: f.clone(), to: PathBuf::from(file_name(f)) });
            if 3 < rank {
                rank = 3;
                home = Some((index, PathBuf::new()));
            }
        }

        for f in &files {
            match ext(f).as_str() {
                "dll" | "asi" => {}
                "cs" | "vb" => {
                    out.add(Kind::Script, stem(f), Action::Copy { from: f.clone(), to: PathBuf::from("scripts").join(file_name(f)) });
                }
                "rpf" if file_name(f).eq_ignore_ascii_case("dlc.rpf") => self.dlc(dir, f, out),
                "rpf" => self.skip(out, f, "Game archive: it can't be dropped in as a file"),
                "oiv" => self.skip(out, f, "OpenIV package: open it with OpenIV's Package Installer"),
                "exe" | "msi" => self.skip(out, f, "Program: run it yourself if it's an installer"),
                "xml" if is_els(f) => {
                    out.add(Kind::Els, stem(f), Action::Copy { from: f.clone(), to: PathBuf::from("ELS").join("pack_default").join(file_name(f)) });
                }
                _ if is_doc(f, top) => self.skip(out, f, "Readme or picture"),
                _ => match &home {
                    Some((index, dest)) => out.attach(*index, Action::Copy { from: f.clone(), to: dest.join(file_name(f)) }),
                    None if lml::data_type(&file_name(f)).is_some() => self.skip(out, f, "Data file without a model: merge it by hand"),
                    None => self.skip(out, f, "Don't know where this goes"),
                },
            }
        }

        for d in subdirs(dir) {
            match &home {
                // A settings or audio folder next to a plugin goes with it.
                Some((index, dest)) if !has_own_content(&d) => {
                    let dest = dest.join(file_name(&d));
                    for f in all_files(&d) {
                        if is_doc(&f, false) {
                            continue;
                        }
                        let rel = f.strip_prefix(&d).unwrap_or(&f).to_path_buf();
                        out.attach(*index, Action::Copy { from: f.clone(), to: dest.join(rel) });
                    }
                }
                _ => self.walk(&d, out),
            }
        }
    }

    /// A folder laid out like the game folder: copied over it as it is.
    fn mirror(&mut self, dir: &Path, out: &mut Out) {
        for f in all_files(dir) {
            let rel = f.strip_prefix(dir).unwrap_or(&f).to_path_buf();
            let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
            let at_top = parts.len() == 1;
            let first = parts[0].as_str();
            let e = ext(&f);
            if first == "mods" {
                // mods\update\x64\dlcpacks\<pack>\dlc.rpf
                if parts.len() == 6 && parts[1..4] == ["update", "x64", "dlcpacks"] && parts[5] == "dlc.rpf" {
                    let pack = f.parent().unwrap_or(dir).to_path_buf();
                    self.dlc(&pack, &f, out);
                } else {
                    self.skip(out, &f, "Edits the game's own archives: install it with OpenIV");
                }
                continue;
            }
            if e == "rpf" && first != "lml" {
                self.skip(out, &f, "Game archive: it can't be dropped in as a file");
                continue;
            }
            if is_doc(&f, at_top) {
                self.skip(out, &f, "Readme or picture");
                continue;
            }
            let kind = match first {
                "plugins" if parts.len() > 2 && parts[1] == "lspdfr" => Kind::LspdfrPlugin,
                "plugins" => Kind::RphPlugin,
                "scripts" => Kind::Script,
                "lspdfr" => Kind::LspdfrFiles,
                "els" => Kind::Els,
                "lml" => Kind::LmlMod,
                _ if at_top && e == "asi" => Kind::Asi,
                _ => Kind::GameFiles,
            };
            let label = match kind {
                Kind::LspdfrPlugin | Kind::RphPlugin | Kind::Script | Kind::Asi if matches!(e.as_str(), "dll" | "asi") => stem(&f),
                Kind::LmlMod if parts.len() > 2 => file_name(&dir.join(rel.components().take(2).collect::<PathBuf>())),
                _ => String::new(),
            };
            out.add(kind, label, Action::Copy { from: f.clone(), to: rel });
        }
    }

    fn dlc(&mut self, dir: &Path, rpf: &Path, out: &mut Out) {
        let name = self.lml_name(dir);
        let root = lml::root(&name);
        let i = out.add(Kind::DlcPack, name.clone(), Action::Copy { from: rpf.to_path_buf(), to: root.join("dlc.rpf") });
        out.attach(i, Action::Write { to: root.join("install.xml"), contents: lml::install_xml(&name, &Package::DlcRpf) });
    }

    /// A car (or other streamed mod) packaged for Lenny's Mod Loader.
    fn vehicle(&mut self, dir: &Path, out: &mut Out) {
        let name = self.lml_name(dir);
        let root = lml::root(&name);
        let all = all_files(dir);
        let lua = all.iter().find(|f| matches!(file_name(f).to_lowercase().as_str(), "fxmanifest.lua" | "__resource.lua"));
        let declared: Vec<(String, String)> = lua.and_then(|l| fs::read_to_string(l).ok()).map(|s| lml::lua_data_files(&s)).unwrap_or_default();
        let has_meta = all.iter().any(|f| lml::data_type(&file_name(f)).is_some() || ext(f) == "meta");

        // Resources with their own audio are left whole for LML to read.
        if let Some(lua) = lua.filter(|_| declared.iter().any(|(k, _)| k.starts_with("AUDIO_"))) {
            let lua_rel = lua.strip_prefix(dir).unwrap_or(lua).to_string_lossy().replace('\\', "/");
            let i = out.add(Kind::AddonVehicle, name.clone(), Action::Write {
                to: root.join("install.xml"),
                contents: lml::install_xml(&name, &Package::Resource { lua: format!("resource/{lua_rel}") }),
            });
            for f in &all {
                if is_doc(f, false) && !is_streamed_or_data(f) {
                    continue;
                }
                let rel = f.strip_prefix(dir).unwrap_or(f);
                out.attach(i, Action::Copy { from: f.clone(), to: root.join("resource").join(rel) });
            }
            return;
        }

        let addon = has_meta || lua.is_some();
        let kind = if addon { Kind::AddonVehicle } else { Kind::ReplaceVehicle };
        let i = out.add(kind, name.clone(), Action::Write { to: PathBuf::new(), contents: String::new() });
        let mut data = Vec::new();
        let mut streaming = false;
        for f in &all {
            let file = file_name(f);
            let rel = f.strip_prefix(dir).unwrap_or(f).to_string_lossy().replace('\\', "/");
            let declared_kind = declared.iter().find(|(_, p)| p.eq_ignore_ascii_case(&rel)).map(|(k, _)| k.clone());
            if lml::is_streamed(f) {
                streaming = true;
                let folder = if addon { "stream" } else { "replace" };
                out.attach(i, Action::Copy { from: f.clone(), to: root.join(folder).join(&file) });
            } else if addon && (declared_kind.is_some() || ext(f) == "meta" || lml::data_type(&file).is_some()) {
                let inner = rel.strip_prefix("data/").unwrap_or(&rel).to_string();
                let path = format!("data/{inner}");
                out.attach(i, Action::Copy { from: f.clone(), to: root.join(path.replace('/', "\\")) });
                let kind = declared_kind.or_else(|| lml::data_type(&file).map(str::to_string));
                data.push(DataFile { kind, path });
            } else if ext(f) == "xml" && is_els(f) {
                out.add(Kind::Els, stem(f), Action::Copy { from: f.clone(), to: PathBuf::from("ELS").join("pack_default").join(&file) });
            } else if matches!(ext(f).as_str(), "lua" | "fxap") {
            } else if matches!(ext(f).as_str(), "awc" | "rel" | "nametable") {
                self.skip(out, f, "Vehicle sound: needs a FiveM-style resource to load");
            } else if is_doc(f, false) {
                self.skip(out, f, "Readme or picture");
            } else {
                self.skip(out, f, "Don't know where this goes");
            }
        }
        data.sort_by_key(|d| data_order(d.kind.as_deref()));
        let package = if addon { Package::Addon { streaming, data } } else { Package::Replace };
        out.parts[i].actions[0] = Action::Write { to: root.join("install.xml"), contents: lml::install_xml(&name, &package) };
    }
}

/// Load order for data files: handling and layouts before the vehicles
/// that use them.
fn data_order(kind: Option<&str>) -> usize {
    match kind {
        Some("HANDLING_FILE") => 0,
        Some("VEHICLE_LAYOUTS_FILE") => 1,
        Some("CARCOLS_FILE") => 2,
        Some("VEHICLE_METADATA_FILE") => 3,
        Some("VEHICLE_VARIATION_FILE") => 4,
        _ => 5,
    }
}

#[derive(Default)]
struct Out {
    parts: Vec<Part>,
    skipped: Vec<Skipped>,
}

impl Out {
    fn add(&mut self, kind: Kind, label: String, action: Action) -> usize {
        self.parts.push(Part { kind, label, actions: vec![action] });
        self.parts.len() - 1
    }

    fn attach(&mut self, index: usize, action: Action) {
        self.parts[index].actions.push(action);
    }

    /// One part per kind for loose game files, and drop duplicate targets.
    fn merge_parts(&mut self) {
        let mut merged: Vec<Part> = Vec::new();
        let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for mut part in std::mem::take(&mut self.parts) {
            part.actions.retain(|a| seen.insert(lower(a.to())));
            if part.actions.is_empty() {
                continue;
            }
            let groupable = matches!(part.kind, Kind::LspdfrFiles | Kind::GameFiles | Kind::Els | Kind::Library | Kind::LspdfrPlugin | Kind::RphPlugin | Kind::Script | Kind::Asi | Kind::LmlMod);
            match by_kind.get(&part.kind) {
                Some(&i) if groupable => {
                    let target = &mut merged[i];
                    if !part.label.is_empty() && !target.label.split(", ").any(|l| l == part.label) {
                        target.label = if target.label.is_empty() { part.label } else { format!("{}, {}", target.label, part.label) };
                    }
                    target.actions.extend(part.actions);
                }
                _ => {
                    by_kind.insert(part.kind, merged.len());
                    merged.push(part);
                }
            }
        }
        self.parts = merged;
    }
}

fn lower(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase().replace('/', "\\"))
}

/// Sets are alternatives when they would install the same files or models.
fn collide(sets: &[Set]) -> bool {
    let keys = |s: &Set| -> HashSet<String> {
        s.parts.iter().flat_map(|p| p.actions.iter()).filter_map(|a| match a {
            Action::Copy { from, to } if lml::is_streamed(from) => Some(format!("model:{}", file_name(from).to_lowercase())),
            Action::Copy { to, .. } => Some(lower(to).to_string_lossy().into_owned()),
            Action::Write { .. } => None,
        }).collect()
    };
    let all: Vec<HashSet<String>> = sets.iter().map(keys).collect();
    all.iter().enumerate().any(|(i, a)| all[i + 1..].iter().any(|b| !a.is_disjoint(b)))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn sorted(dir: &Path, want_dirs: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).filter(|e| e.file_type().map(|t| t.is_dir() == want_dirs).unwrap_or(false)).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn files(dir: &Path) -> Vec<PathBuf> {
    sorted(dir, false)
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    sorted(dir, true)
}

fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()).map(|e| e.into_path()).collect();
    v.sort();
    v
}

/// Steps into wrapper folders: a folder whose only content is one folder.
fn unwrap(root: &Path) -> PathBuf {
    let mut dir = root.to_path_buf();
    loop {
        let dirs = subdirs(&dir);
        let real_files = files(&dir).iter().filter(|f| !is_doc(f, true)).count();
        match dirs.as_slice() {
            [only] if real_files == 0 && !is_mirror(&dir) && !is_game_folder(only) && !vehicle_root(&dir) => {
                dir = only.clone();
            }
            _ => return dir,
        }
    }
}

fn is_mirror(dir: &Path) -> bool {
    subdirs(dir).iter().any(|d| is_game_folder(d))
        || files(dir).iter().any(|f| GAME_ROOT_FILES.contains(&file_name(f).to_lowercase().as_str()) || ext(f) == "asi")
}

/// A folder named like one in the game folder. An `ELS` folder only counts
/// with `pack_default` inside, since car downloads use "ELS" for a version.
fn is_game_folder(d: &Path) -> bool {
    match file_name(d).to_lowercase().as_str() {
        "els" => d.join("pack_default").is_dir(),
        name => GAME_FOLDERS.contains(&name),
    }
}

/// A folder holding one vehicle: models here or in `stream\`, or a FiveM
/// resource manifest.
fn vehicle_root(dir: &Path) -> bool {
    let here = files(dir);
    if here.iter().any(|f| matches!(file_name(f).to_lowercase().as_str(), "fxmanifest.lua" | "__resource.lua")) {
        return true;
    }
    here.iter().any(|f| lml::is_streamed(f))
        || subdirs(dir).iter().any(|d| file_name(d).eq_ignore_ascii_case("stream") && all_files(d).iter().any(|f| lml::is_streamed(f)))
}

fn is_els(f: &Path) -> bool {
    fs::read(f).map(|b| String::from_utf8_lossy(&b[..b.len().min(4096)]).to_lowercase().contains("<vcfroot")).unwrap_or(false)
}

fn is_streamed_or_data(f: &Path) -> bool {
    lml::is_streamed(f) || ext(f) == "meta"
}

/// Readmes and pictures. Pictures and loose text only count at the top of
/// the download; deeper down they may be used by the mod.
fn is_doc(f: &Path, top: bool) -> bool {
    let e = ext(f);
    if !DOCS.contains(&e.as_str()) {
        return false;
    }
    let name = stem(f).to_lowercase();
    let readme = ["readme", "read me", "read_me", "license", "licence", "changelog", "credits", "instructions", "install", "how to", "terms"].iter().any(|w| name.contains(w));
    top || readme || matches!(e.as_str(), "url" | "pdf" | "md" | "nfo" | "doc" | "docx" | "rtf" | "psd" | "mp4")
}

fn is_installable(f: &Path) -> bool {
    matches!(ext(f).as_str(), "dll" | "asi" | "cs" | "vb" | "rpf") || lml::is_streamed(f) || (ext(f) == "xml" && is_els(f))
}

/// Something the installer acts on, somewhere in this folder.
fn has_content(dir: &Path) -> bool {
    is_mirror(dir) || all_files(dir).iter().any(|f| is_installable(f))
}

/// Content that is a mod in its own right, not a plugin's settings folder.
fn has_own_content(dir: &Path) -> bool {
    is_mirror(dir) || all_files(dir).iter().any(|f| matches!(ext(f).as_str(), "dll" | "asi" | "rpf") || lml::is_streamed(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree(files: &[(&str, &[u8])]) -> Tmp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("lspdfr-installer-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, data) in files {
            let p = root.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        Tmp(root)
    }

    fn lspdfr_dll() -> Vec<u8> {
        dll(&["LSPD First Response", "Plugin"])
    }

    fn dll(strings: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x98..0x9a].copy_from_slice(&0x10bu16.to_le_bytes());
        let clr = 0x98 + 96 + 14 * 8;
        b[clr..clr + 4].copy_from_slice(&0x2008u32.to_le_bytes());
        b.push(0);
        for s in strings {
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        b
    }

    fn targets(plan: &Plan) -> Vec<String> {
        let mut v: Vec<String> = plan.parts().flat_map(|p| p.actions.iter()).map(|a| a.to().to_string_lossy().replace('\\', "/")).collect();
        v.sort();
        v
    }

    #[test]
    fn copies_a_game_folder_layout_as_it_is() {
        let t = tree(&[
            ("Callouts v1.2/Grand Theft Auto V/plugins/LSPDFR/Callouts.dll", &lspdfr_dll()),
            ("Callouts v1.2/Grand Theft Auto V/plugins/LSPDFR/Callouts.ini", b"[x]"),
            ("Callouts v1.2/Grand Theft Auto V/lspdfr/audio/scanner/a.wav", b"w"),
            ("Callouts v1.2/Grand Theft Auto V/Lib.dll", b"MZ"),
            ("Callouts v1.2/README.txt", b"hi"),
        ]);
        let plan = analyze(&t.0, "Callouts");
        assert_eq!(plan.sets.len(), 1);
        assert!(!plan.needs_decision());
        assert_eq!(targets(&plan), ["Lib.dll", "lspdfr/audio/scanner/a.wav", "plugins/LSPDFR/Callouts.dll", "plugins/LSPDFR/Callouts.ini"]);
        assert!(plan.sets[0].skipped.is_empty());
    }

    #[test]
    fn places_loose_plugins_by_what_they_are() {
        let t = tree(&[
            ("StopThePed.dll", &lspdfr_dll()),
            ("StopThePed.ini", b"[x]"),
            ("StopThePed/config.xml", b"<x/>"),
            ("RAGENativeUI.dll", &dll(&["RagePluginHook", "PluginAttribute"])),
            ("Trainer.dll", &dll(&["ScriptHookVDotNet3", "Script"])),
        ]);
        let plan = analyze(&t.0, "StopThePed");
        assert_eq!(targets(&plan), [
            "RAGENativeUI.dll",
            "plugins/LSPDFR/StopThePed.dll",
            "plugins/LSPDFR/StopThePed.ini",
            "plugins/LSPDFR/StopThePed/config.xml",
            "scripts/Trainer.dll",
        ]);
    }

    #[test]
    fn packages_a_fivem_car_for_the_mod_loader() {
        let t = tree(&[
            ("JRUFictional/stream/JointKent.yft", b"m"),
            ("JRUFictional/stream/JointKent.ytd", b"t"),
            ("JRUFictional/vehicles.meta", b"<x/>"),
            ("JRUFictional/handling.meta", b"<x/>"),
            ("JRUFictional/__resource.lua", b"data_file 'HANDLING_FILE' 'handling.meta'\ndata_file 'VEHICLE_METADATA_FILE' 'vehicles.meta'"),
        ]);
        let plan = analyze(&t.0, "JRU");
        assert_eq!(targets(&plan), [
            "lml/JRUFictional/data/handling.meta",
            "lml/JRUFictional/data/vehicles.meta",
            "lml/JRUFictional/install.xml",
            "lml/JRUFictional/stream/JointKent.yft",
            "lml/JRUFictional/stream/JointKent.ytd",
        ]);
        let xml = plan.parts().flat_map(|p| &p.actions).find_map(|a| match a { Action::Write { contents, .. } => Some(contents.clone()), _ => None }).unwrap();
        assert!(xml.contains("<DataFile type=\"HANDLING_FILE\">data/handling.meta</DataFile>"));
        assert!(xml.find("HANDLING_FILE").unwrap() < xml.find("VEHICLE_METADATA_FILE").unwrap());
        assert_eq!(plan.kinds(), [Kind::AddonVehicle]);
    }

    #[test]
    fn replacement_cars_and_their_els_file() {
        let t = tree(&[
            ("2016 Explorer/police3.yft", b"m"),
            ("2016 Explorer/police3_hi.yft", b"m"),
            ("2016 Explorer/police3.ytd", b"t"),
            ("2016 Explorer/POLICE3.xml", b"<?xml version=\"1.0\"?>\n<vcfroot Description=\"x\">"),
        ]);
        let plan = analyze(&t.0, "Explorer");
        assert_eq!(plan.kinds(), [Kind::ReplaceVehicle, Kind::Els]);
        assert!(targets(&plan).contains(&"lml/2016 Explorer/replace/police3.yft".to_string()));
        assert!(targets(&plan).contains(&"ELS/pack_default/POLICE3.xml".to_string()));
    }

    #[test]
    fn alternative_versions_need_a_choice() {
        let t = tree(&[
            ("Pack/ELS/police.yft", b"m"),
            ("Pack/ELS/POLICE.xml", b"<vcfroot>"),
            ("Pack/Non-ELS/police.yft", b"m"),
        ]);
        let mut plan = analyze(&t.0, "Pack");
        assert!(plan.exclusive);
        assert!(plan.needs_decision());
        assert_eq!(plan.sets.iter().filter(|s| s.selected).count(), 1);
        plan.select(1, true);
        assert!(!plan.sets[0].selected && plan.sets[1].selected);
    }

    #[test]
    fn separate_cars_install_together_and_optional_extras_wait() {
        let t = tree(&[
            ("Pack/Charger/stream/charger.yft", b"m"),
            ("Pack/Charger/vehicles.meta", b"<x/>"),
            ("Pack/Tahoe/stream/tahoe.yft", b"m"),
            ("Pack/Tahoe/vehicles.meta", b"<x/>"),
            ("Pack/Optional/stream/tahoe2.yft", b"m"),
            ("Pack/Optional/vehicles.meta", b"<x/>"),
        ]);
        let plan = analyze(&t.0, "Pack");
        assert!(!plan.exclusive);
        let chosen: Vec<&str> = plan.sets.iter().filter(|s| s.selected).map(|s| s.label.as_str()).collect();
        assert_eq!(chosen, ["Charger", "Tahoe"]);
    }

    #[test]
    fn dlc_packs_load_through_the_mod_loader() {
        let t = tree(&[("mods/update/x64/dlcpacks/lapd/dlc.rpf", b"RPF7"), ("mods/update/update.rpf", b"RPF7")]);
        let plan = analyze(&t.0, "LAPD");
        assert_eq!(targets(&plan), ["lml/lapd/dlc.rpf", "lml/lapd/install.xml"]);
        assert_eq!(plan.sets[0].skipped[0].reason, "Edits the game's own archives: install it with OpenIV");
    }

    #[test]
    fn script_hook_v_from_its_bin_folder() {
        let t = tree(&[("bin/ScriptHookV.dll", b"MZ"), ("bin/dinput8.dll", b"MZ"), ("bin/NativeTrainer.asi", b"MZ"), ("readme.txt", b"x")]);
        let plan = analyze(&t.0, "ScriptHookV");
        assert_eq!(targets(&plan), ["NativeTrainer.asi", "ScriptHookV.dll", "dinput8.dll"]);
    }
}
