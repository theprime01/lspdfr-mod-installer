//! GitHub releases: one-click downloads of the essentials hosted there, and
//! updates of this app.

use std::io::Read;
use std::path::{Path, PathBuf};

pub const REPO: &str = "theprime01/lspdfr-mod-installer";

pub struct Release {
    pub tag: String,
    /// (file name, download URL)
    pub assets: Vec<(String, String)>,
}

pub fn latest(repo: &str) -> Result<Release, String> {
    let json: serde_json::Value = ureq::get(&format!("https://api.github.com/repos/{repo}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "lspdfr-mod-installer")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().ok_or("no release found")?.to_string();
    let assets = json["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["browser_download_url"].as_str()?.to_string()))).collect())
        .unwrap_or_default();
    Ok(Release { tag, assets })
}

pub fn download(url: &str, to: &Path) -> Result<(), String> {
    let response = ureq::get(url).set("User-Agent", "lspdfr-mod-installer").call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response.into_reader().take(500 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(to, bytes).map_err(|e| e.to_string())
}

/// Downloads a release file of a GitHub repo, e.g. RAGENativeUI.zip.
pub fn fetch(repo: &str, asset: &str, dir: &Path) -> Result<PathBuf, String> {
    let release = latest(repo)?;
    let (name, url) = release.assets.iter().find(|(n, _)| n.eq_ignore_ascii_case(asset)).ok_or_else(|| format!("{asset} isn't in the latest {repo} release"))?;
    let to = dir.join(name);
    download(url, &to)?;
    Ok(to)
}

/// True when `tag` (e.g. "v0.2.0") is a newer version than `current`.
pub fn newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect() };
    parse(tag) > parse(current)
}

/// The app's newest release, if it's newer than this one: (version, zip URL).
pub fn update_available() -> Option<(String, String)> {
    let release = latest(REPO).ok()?;
    if !newer(&release.tag, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let (_, url) = release.assets.iter().find(|(n, _)| n.ends_with("windows-x64.zip"))?;
    Some((release.tag.trim_start_matches('v').to_string(), url.clone()))
}

/// Downloads the new version over this exe and starts it. The old exe is
/// moved aside (Windows lets a running exe be renamed) and deleted on the
/// next start.
pub fn update(url: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let zip_path = std::env::temp_dir().join("lspdfr-mod-installer-update.zip");
    download(url, &zip_path)?;
    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let index = (0..zip.len()).find(|&i| zip.by_index(i).is_ok_and(|f| f.name().to_lowercase().ends_with(".exe"))).ok_or("no exe in the download")?;
    let mut bytes = Vec::new();
    zip.by_index(index).map_err(|e| e.to_string())?.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip_path);
    let new = exe.with_extension("new");
    std::fs::write(&new, bytes).map_err(|e| e.to_string())?;
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    std::process::Command::new(&exe).spawn().map_err(|e| e.to_string())?;
    std::process::exit(0);
}

/// Deletes the exe an update left behind.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compares_versions() {
        assert!(super::newer("v0.2.0", "0.1.0"));
        assert!(super::newer("v0.10.0", "0.9.3"));
        assert!(!super::newer("v0.1.0", "0.1.0"));
        assert!(!super::newer("v0.1.0", "0.2.0"));
    }
}
