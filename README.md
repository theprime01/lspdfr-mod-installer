# LSPDFR Mod Installer

A drag and drop installer for GTA V and LSPDFR mods. Drop a download on the window and it works out what the mod is, puts every file in the right place, and remembers it, so you can switch it off, put it in a profile or remove it again later.

It handles LSPDFR plugins and callouts, RAGE Plugin Hook plugins, scripts, `.asi` mods, add-on and replacement cars, DLC packs, ELS lighting, and EUP uniforms.

> Works with **GTA V Legacy** (the version LSPDFR runs on). GTA V Enhanced isn't supported.

## Quick start

1. Download the latest `lspdfr-mod-installer-…-windows-x64.zip` from [Releases](https://github.com/theprime01/lspdfr-mod-installer/releases), unzip it anywhere and run **LSPDFR Mod Installer.exe**.
2. It finds your GTA V folder on its own (Steam, Rockstar launcher or Epic). If it picks the wrong one, press **Change...** at the top.
3. Open the **Setup** tab and install anything marked **Missing**. RAGENativeUI, the ASI loader and Script Hook V .NET install with one click; the others have a download page button.
4. Drag a mod's download (`.zip`, `.rar`, `.7z`, a folder or a single file) onto the window. Check what it's going to do, then press **Install**.
5. Start the game with **Start LSPDFR** at the top, not from Steam.

## What you need

| | |
|---|---|
| **RAGE Plugin Hook** and **LSPDFR** | LSPDFR itself. RAGE Plugin Hook comes with it. |
| **RAGENativeUI** | Menus used by most plugins. |
| **Lenny's Mod Loader (LML)** | Loads cars, DLC packs and EUP without editing the game's archives. |
| **Script Hook V** + ASI loader | For `.asi` mods like ELS and trainers. |
| **Script Hook V .NET** | Only if you have scripts for the `scripts` folder. |
| **ELS** | Only for cars that come with an ELS `.xml`. |
| **EUP Law & Order** + **EUP Menu** | Only for police uniforms. |

The Setup tab shows which of these you have.

## What goes where

| What you drop | Where it goes |
|---|---|
| LSPDFR plugin or callout `.dll` | `plugins\LSPDFR\`, with its `.ini` and settings folders |
| RAGE Plugin Hook plugin `.dll` | `plugins\`, plus a line in `startup.rphs` so it loads when the game starts |
| Script Hook V .NET script (`.dll`, `.cs`, `.vb`) | `scripts\` |
| `.asi` mods and libraries (RAGENativeUI.dll, NAudio, …) | The game folder |
| Add-on car (models plus `vehicles.meta` etc., or a FiveM resource) | An LML package in `lml\<car>\` |
| Replacement car (only `.yft` / `.ytd` files) | An LML replacement package in `lml\<car>\replace\` |
| DLC pack (`dlc.rpf`) | An LML package that loads it, so `dlclist.xml` never needs editing |
| ELS `.xml` | `ELS\pack_default\` |
| A ready-made LML package (a folder with its own `install.xml`) | Copied into `lml\` as it is |
| EUP uniforms and add-on packs | See [EUP](#eup) |
| A "Grand Theft Auto V" folder layout (`plugins\`, `lspdfr\`, …) | Copied over the game folder as laid out |

Some downloads come in versions (for example `ELS\` and `Non-ELS\` folders): you choose which one. Folders named `Optional` or `Extras` are left unticked, apart from ones the author marks **recommended**. Anything the installer can't place is listed under **Not installed** with the reason, such as readmes, `.oiv` packages (install those with OpenIV) and texture pictures.

A `.dll` or `.asi` is never swapped for an older version. For example, EUP Menu comes with RAGENativeUI 1.8.1, and a 1.9.3 you already have stays.

## Profiles

Profiles are sets of mods you switch between, like **American** and **British** LSPDFR. **Set up American and British** in the **Profile** bar at the top makes both, and you switch profiles there too.

- A new mod goes into the profile that's on. Change a mod's profiles on the **Installed** tab.
- Essentials (LSPDFR, RAGE Plugin Hook, RAGENativeUI, LML, Script Hook V, EUP, EUP Menu and the adjusters) go into every profile.
- Switching profile switches the other profile's mods off and this one's on. Nothing is deleted.

## Switching mods off

The tick next to each mod on the **Installed** tab switches it off without removing it. That's the quickest way to find the mod that's breaking things. Switched-off files wait in `LSPDFR Mod Installer\switched off\` inside the game folder, which the game doesn't read.

**Clean game** (at the top) switches every mod off at once, for GTA Online, and puts them all back when you switch it off again.

Plugins switched off are also taken out of `startup.rphs`, and come back when they're switched on. Lines you wrote in `startup.rphs` yourself are never touched.

## EUP

The **EUP** tab shows EUP Law & Order, Serve & Rescue, EUP Menu (with the key that opens it, F11 by default, and a button to change it), the memory adjusters, and your add-on packs, each with an on/off switch.

1. **EUP Law & Order:** download the **LML version** and drop the zip on the Install tab.
2. **EUP Menu:** drop its zip too. It loads automatically when the game starts.
3. **Add-on packs** (for example British police uniforms): drop them on the Install tab as well.

How it handles them:

- **Add-on packs load after EUP**, so their clothing replaces EUP's.
- **Outfits that come with a pack** (its `wardrobe.ini`) get their own section in EUP Menu, named after the pack, for example *EUP → Santos Constabulary*. They don't replace the wardrobe you save outfits in, and the section goes when the pack is switched off.
- **Retexture packs replace EUP's American uniforms.** The game has one texture per piece of clothing, so a British retexture of a shirt means that shirt is British for everyone. Put the pack in your **British** profile only, and switching to **American** brings the US uniforms back.
- **EUP's own LSPDFR outfit files are left out.** They're from 2021, and LSPDFR 0.4.9 crashes the game when it dresses cops with them. LSPDFR's police wear LSPDFR's outfits, and you put EUP uniforms on with EUP Menu.
- **Old packs:** a pack made for an older EUP than 8.3 puts its textures on the wrong clothes, because EUP moved its clothing around between versions. The Install tab warns when a pack's readme says it's for an older EUP. Look for a version made for EUP 8.3.

## Troubleshooting

**Start with the Crash help tab.** It reads the last session's `RagePluginHook.log`, names the plugin that crashed and offers to switch it off.

**The game crashes as soon as it has loaded, or a few seconds in, with no error**
- If you have no scripts in the `scripts` folder, switch **ScriptHookVDotNet** off on the Installed tab. It can crash the game while it starts up, even with nothing to run.
- Otherwise, switch off the mods you added most recently, then switch them back on one at a time.

**The game crashes when cops spawn, after installing EUP**
- EUP installed by hand (not with this app) brings LSPDFR outfit files that crash LSPDFR 0.4.9. Remove EUP and drop the LML version on the Install tab, and the app leaves those files out.
- With lots of cars and clothing, install **Heap Adjuster** and **Packfile Limit Adjuster** (on the EUP and Setup tabs).

**EUP Menu says EUP Law & Order isn't installed**
- Use the **LML version** of EUP Law & Order, and check LML shows as installed on the Setup tab.
- EUP Menu opens with **F11** by default. **Change the key** on the EUP tab shows the setting.

**Uniforms look wrong, or textures are on the wrong clothes**
- The pack was probably made for an older EUP. The Install tab would have warned you. Switch the pack off and look for an EUP 8.3 version.

**A car doesn't spawn**
- Use the car's **spawn name**: it's listed under the mod on the Installed tab (click to copy). In game, open the RAGE Plugin Hook console with **F4** and type `spawn <name>`.
- Cars, DLC packs and EUP need **Lenny's Mod Loader** (Setup tab).
- Check the car's mod is switched on and in the profile that's on.

**A car's emergency lights only partly work**
- ELS lights come on in stages: press **J** to step through them. Many cars only light some lights at each stage on purpose.
- Some cars come with a plugin for extra lights (cruise or park lights). The installer ticks recommended ones; check it's installed and switched on.

**A plugin doesn't load**
- Open the RAGE Plugin Hook console with **F4** and type `LoadPlugin "<name>.dll"` to see the error.
- Check `startup.rphs` in the game folder has a `LoadPlugin` line for it. Plugins installed with this app get one automatically.

**After a GTA V update, nothing works**
- RAGE Plugin Hook and Script Hook V have to support the new game version. Wait for their updates, then drop the new versions on the Install tab. A banner at the top warns after a game update.

**Mods don't run at all**
- Start the game with **Start LSPDFR**, not from Steam or the Rockstar launcher.
- Turn BattlEye off for GTA V in the Rockstar Games Launcher settings. Story mode mods don't run with it on.

**A mod I installed by hand isn't on the Installed tab**
- The app only knows about mods it installed. Remove the hand-installed files, then drop the download on the Install tab.

## Safety

- Every installed file is recorded. Any file it replaces is backed up to `%APPDATA%\LSPDFR Mod Installer\backups`, and **Remove** puts the originals back.
- Dropping a newer version of a mod replaces the old one, and it keeps its profiles.
- The game's own files (`*.rpf`, `update\`, `x64\`, `GTA5.exe`, …) are never written to.
- Installing is refused while GTA V or RAGE Plugin Hook is running.
- Settings and the installed-mods list are in `%APPDATA%\LSPDFR Mod Installer`.

## Building

```powershell
cargo test
cargo build --release   # target\release\lspdfr-mod-installer.exe
```

`lspdfr-mod-installer.exe --plan <zip or folder>` prints where everything would go, without installing.

## License

MIT, see [LICENSE](LICENSE).
