//! Unpacks dropped archives (zip, rar, 7z) into a staging folder, including
//! archives nested inside them.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

const NESTED_DEPTH: usize = 3;

pub fn is_archive(path: &Path) -> bool {
    matches!(ext(path).as_str(), "zip" | "rar" | "7z")
}

pub fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Extracts `archive` into `dest`, then any archives found inside it, each
/// into a folder named after it.
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    extract(archive, dest)?;
    for _ in 0..NESTED_DEPTH {
        let nested: Vec<PathBuf> = walkdir::WalkDir::new(dest)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_archive(e.path()))
            .map(|e| e.into_path())
            .collect();
        if nested.is_empty() {
            break;
        }
        for inner in nested {
            let folder = inner.with_extension("");
            let folder = if folder.exists() { inner.with_extension("unpacked") } else { folder };
            extract(&inner, &folder).map_err(|e| format!("{}: {e}", inner.file_name().unwrap_or_default().to_string_lossy()))?;
            let _ = fs::remove_file(&inner);
        }
    }
    Ok(())
}

pub fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    match ext(archive).as_str() {
        "zip" => zip(archive, dest),
        "7z" => seven(archive, dest),
        "rar" => rar(archive, dest),
        other => Err(format!("can't open .{other} files")),
    }
}

/// `name` as a path inside `root`, or None if it would land outside it.
pub fn inside(root: &Path, name: &Path) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for c in name.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != root).then_some(out)
}

fn zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(io::BufReader::new(file)).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        let Some(out) = inside(dest, &name) else { continue };
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut w = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        io::copy(&mut entry, &mut w).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn seven(archive: &Path, dest: &Path) -> Result<(), String> {
    sevenz_rust2::decompress_file_with_extract_fn(archive, dest, |entry, reader, path| {
        if !path.starts_with(dest) || path.components().any(|c| c == Component::ParentDir) {
            return Ok(true);
        }
        sevenz_rust2::default_entry_extract_fn(entry, reader, path)
    })
    .map_err(|e| e.to_string())
}

fn rar(archive: &Path, dest: &Path) -> Result<(), String> {
    let mut open = unrar::Archive::new(archive).open_for_processing().map_err(|e| e.to_string())?;
    while let Some(header) = open.read_header().map_err(|e| e.to_string())? {
        let entry = header.entry();
        let target = if entry.is_file() { inside(dest, &entry.filename) } else { None };
        open = match target {
            Some(out) => {
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                header.extract_to(&out).map_err(|e| e.to_string())?
            }
            None => header.skip().map_err(|e| e.to_string())?,
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_cannot_escape_the_staging_folder() {
        let root = Path::new(r"C:\staging\1");
        assert_eq!(inside(root, Path::new("a/b.dll")), Some(PathBuf::from(r"C:\staging\1\a\b.dll")));
        assert_eq!(inside(root, Path::new("../evil.dll")), None);
        assert_eq!(inside(root, Path::new(r"C:\Windows\evil.dll")), None);
        assert_eq!(inside(root, Path::new("")), None);
    }

    #[test]
    fn unpacks_7z() {
        let root = std::env::temp_dir().join(format!("lspdfr-7z-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src").join("Callouts");
        std::fs::create_dir_all(src.join("plugins").join("LSPDFR")).unwrap();
        std::fs::write(src.join("plugins").join("LSPDFR").join("Callouts.dll"), "MZ").unwrap();
        let archive = root.join("callouts.7z");
        sevenz_rust2::compress_to_path(root.join("src"), &archive).unwrap();
        unpack(&archive, &root.join("out")).unwrap();
        let found: Vec<_> = walkdir::WalkDir::new(root.join("out")).into_iter().flatten().filter(|e| e.file_name() == "Callouts.dll").collect();
        assert_eq!(found.len(), 1);
        assert_eq!(std::fs::read(found[0].path()).unwrap(), b"MZ");
        let _ = std::fs::remove_dir_all(&root);
    }
}
