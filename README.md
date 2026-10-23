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

## Profiles: American, British, ...

The **Profile** bar at the top switches between sets of mods, e.g. an American and a British LSPDFR. **Set up American and British** makes both. Any other name works too with **+ New profile**, and right-clicking a profile deletes it.

- Mods you install go into the profile that's on. Essentials (LSPDFR, RAGE Plugin Hook, RAGENativeUI, LML, Script Hook V, ASI loaders) and plain libraries go into **every profile**.
- On the Installed tab, the ticks under each mod change which profiles it's in.
- Switching profiles moves the other profile's files out of the game folder and puts this one's back. When two profiles replace the same file (a UK and a US scanner audio pack, say), each profile gets its own copy.
- **All mods** switches everything on.

## Clean game

**Clean game** switches off every mod installed with this app, for GTA Online or a clean story mode. Nothing is deleted: switching it off puts back exactly what was on. Mods installed by hand aren't touched.

## Switching mods off

The tick next to each mod on the Installed tab switches it off without removing it, handy for finding the mod that breaks things. Switched-off files wait in `LSPDFR Mod Installer\switched off\` inside the game folder, which the game, RAGE Plugin Hook, Script Hook V and LML don't read.

## Crash help

The **Crash help** tab reads `RagePluginHook.log` from the last session. It names the plugin that crashed, matches it to the installed mod, and offers to switch it off or remove it. When a plugin is missing a file like `RAGENativeUI.dll`, it says so, with a one-click install where one exists.

## More

- **Spawn names**: each car mod lists the model names to spawn it with. Click one to copy it.
- **Settings**: a mod's `.ini` and config files open in a built-in editor from the Installed tab.
- **Conflicts**: before installing, a mod shows which other mods' files it would replace. One that replaces another mod's files (not just shared libraries) waits for you to press Install.
- **One-click installs** on the Setup tab for RAGENativeUI, Ultimate ASI Loader and Script Hook V .NET, from their GitHub releases. LSPDFR, Script Hook V and LML need their download pages.
- **Start LSPDFR** starts `RAGEPluginHook.exe`.
- **GTA updates**: after a game update, a banner warns that RPH, LSPDFR and Script Hook V may need updating.
- **App updates**: a banner offers new versions of this installer, and **Update now** installs them.
- **Export list...** saves your mod list as a text file.

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
