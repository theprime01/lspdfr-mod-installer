# LSPDFR Mod Installer

Drag and drop installer for GTA V / LSPDFR mods. Drop a download (zip, rar, 7z), a folder, or a single file on the window (or on the exe's icon). The installer works out what it is and puts every file where it belongs.

| What you drop | Where it goes |
|---|---|
| A "Grand Theft Auto V" folder layout (`plugins\`, `scripts\`, `lspdfr\`, `ELS\pack_default\`, `.asi` files, …) | Copied over the game folder as laid out |
| LSPDFR plugin / callout `.dll` | `plugins\LSPDFR\`, with its `.ini` and settings folders |
| RAGE Plugin Hook plugin `.dll` | `plugins\` |
| Script Hook V .NET script (`.dll`, `.cs`, `.vb`) | `scripts\` |
| `.asi` mods, libraries (RAGENativeUI.dll, NAudio, …) | The game folder |
| Add-on car (models + `vehicles.meta` etc., or a FiveM resource with `fxmanifest.lua` / `__resource.lua`) | A Lenny's Mod Loader package in `lml\<car>\` with a generated `install.xml` |
| Replacement car (only `.yft` / `.ytd`) | An LML replacement package in `lml\<car>\replace\` |
| `dlc.rpf` (also `mods\update\x64\dlcpacks\<pack>\dlc.rpf`) | An LML package that loads the DLC, so `dlclist.xml` never needs editing |
| ELS `.xml` (`<vcfroot>`) | `ELS\pack_default\` |
| Script Hook V, LSPDFR, RPH, LML downloads themselves | Installed the same way |

Plugin DLLs are told apart by the .NET names they reference: LSPDFR's `Plugin`, RPH's `PluginAttribute`, SHVDN's `Script`.

Downloads with several versions (e.g. `ELS\` and `Non-ELS\` folders) ask which one to install. Folders named `Optional`, `Extras` and the like are left out, with an **Install this too** button. Anything the installer can't place is listed under **Not installed** with the reason: readmes, `.oiv` packages (use OpenIV), and edits inside the game's own archives.

## Safety

- Every installed file is recorded, and any file it replaces is backed up to `%APPDATA%\LSPDFR Mod Installer\backups`. **Remove** on the Installed tab deletes the mod's files and puts the originals back. Files that another, later mod also installed stay, and that mod takes over the original's backup.
- Dropping a newer version of a mod with the same name replaces the old one.
- The game's own files (`*.rpf`, `update\`, `x64\`, `GTA5.exe`, …) are never written to.
- Installing is refused while GTA V or RAGE Plugin Hook is running.

## Setup tab

Shows whether RAGE Plugin Hook, LSPDFR, RAGENativeUI, Lenny's Mod Loader, Script Hook V, the ASI loader, Script Hook V .NET, OpenIV.asi and ELS are installed, with download pages. A dropped mod warns if something it needs is missing.

## Building

```powershell
cargo test
cargo build --release   # target\release\lspdfr-mod-installer.exe
```

`lspdfr-mod-installer.exe --plan <zip or folder>` prints where everything would go, without installing (debug build, from a console).

The game folder is found from the Rockstar launcher, Steam and Epic, or chosen with **Change...**. Settings and the installed-mods list are in `%APPDATA%\LSPDFR Mod Installer`.

## License

MIT, see [LICENSE](LICENSE).
