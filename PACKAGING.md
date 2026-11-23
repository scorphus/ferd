# Packaging an eframe app for Windows, Linux and macOS: notes from other projects

Written from other projects and their sessions up to 2026-11-22, for ferd (https://github.com/scorphus/ferd, branch `feat/windows`). They and ferd both start through `eframe::run_native` on eframe 0.37 with the wgpu renderer, so the build, packaging and release steps below carry over. Parts that do not apply to ferd are left out.

Each statement is marked by where it comes from: **measured** (seen on a real machine or in a CI run; "the owner reported" means Pablo saw it on his machines), **read** (read in source, commits or docs), **not verified**.

## The rule: do as Zed does

The owner's standing rule for packaging is "do as Zed does" (his words, 2026-11-21): copy what Zed (https://github.com/zed-industries/zed) ships and how it builds it, unless a part serves a Zed feature the app lacks. **Read**, from the other project's files below, what that gives per platform:

- macOS: one `.dmg` per CPU, built with Zed's fork of cargo-bundle, holding the `.app` next to a link to `/Applications`.
- Windows: one Inno Setup installer per CPU (x86_64 and Arm), for the current user only, with an uninstaller listed in Settings > Apps.
- Linux: one `.tar.gz` per CPU holding an `myapp.app/` folder with the program, the libraries it needs, the icon, the menu entry, and `install.sh` / `uninstall.sh` into `~/.local`.
- Release build settings from Zed: workspace `Cargo.toml` has `[profile.release] lto = "thin"`, `codegen-units = 1`; `.cargo/bundle-config.toml`, passed with `cargo --config .cargo/bundle-config.toml` only by the packaging builds, sets `opt-level = "z"` for the app's own crates and 3 for dependencies (commit 85f2309).
- Left out of Zed's way, with the reason (read in the session's Zed comparison of 2026-11-21): signing with a Developer ID and notarizing (needs Apple's paid certificate), Zed's updater, `AppUserModelID` on the Windows shortcuts (needs the app to set the same ID at run time; winit does not).

## Windows

### What the release build produces (read: `.github/workflows/release.yml`, `packaging/windows/myapp.iss`)

- Two installers, `MyApp-x86_64.exe` and `MyApp-aarch64.exe`, built on `windows-latest` from `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` (commit 1d59391). The `.iss` takes `/DVersion=` and `/DArch=`; Arch `aarch64` sets `ArchitecturesAllowed=arm64`, otherwise `x64compatible`.
- Inno Setup comes from `choco install innosetup`. ISCC runs from a bash step with `MSYS_NO_PATHCONV: "1"`; without it Git Bash turns the `/D…` options into file paths and ISCC fails with "You may not specify more than one script filename" (**measured**, release run 44914287599).
- Per-user install: `PrivilegesRequired=lowest`, `DefaultDirName={localappdata}\Programs\MyApp`, `MinVersion=10.0.16299` (Windows 10 1709, commit 0c41aa3), `AppVerName`, `VersionInfoVersion`, `AppSupportURL`, `AppUpdatesURL` (commit 40e2bc1).
- The uninstaller removes the program and the cache folder, then asks separately whether to remove the settings folder and the user's documents folder, default No (`[Code]` section of the `.iss`).
- `licenses.md` is installed next to the exe.

### A running app during install (read: commit 99f6eb1)

`crates/myapp/src/main.rs` creates a named mutex `MyApp-Instance-Mutex` with `windows_sys::Win32::System::Threading::CreateMutexW` (dependency `windows-sys = 0.61`, features `Win32_Security`, `Win32_System_Threading`). The `.iss` names the same mutex in `AppMutex` and `SetupMutex`, with `CloseApplications=force`, so the installer sees the app running and closes it before replacing the exe. **Measured**: the owner reported the installer closing a running app works ("that's tested", 2026-11-21).

### Exe details (read)

- `build.rs` uses the `winresource` build dependency to put the icon (`assets/AppIcon.ico`) and `ProductName`, `FileDescription`, `CompanyName` (from `CARGO_PKG_AUTHORS`) and `LegalCopyright` (the `Copyright` line of `LICENSE`) into the exe (commits 93b055c, 40e2bc1). winresource's defaults put the crate name in `ProductName` and `FileDescription`, which is what Task Manager shows.
- `.cargo/config.toml` builds the C runtime into the exe on MSVC: `[target.'cfg(all(target_os = "windows", target_env = "msvc"))'] rustflags = ["-C", "target-feature=+crt-static"]` (commit 3ee2d85). **Measured**: without it the installed exe stopped with "VCRUNTIME140.dll was not found" on the owner's laptop; with it, "it works now on windows".
- `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]` gives a windowed release exe and a console debug exe (commit ae30ddf).

### Seeing errors (measured on the owner's laptop, 2026-11-18)

- The windowed exe prints nothing. A debug build was cross-built from the Mac for `x86_64-pc-windows-gnu` (449 MB, console) and run as `$env:RUST_LOG="debug"; $env:RUST_BACKTRACE=1; .\myapp-debug.exe`. The owner reported Vulkan messages in its log; with `$env:WGPU_BACKEND="dx12"` the app opened.
- The debug build prints a Khronos validation-layer message ("unable to find layer VK_LAYER_KHRONOS_validation"); it comes from the debug build asking wgpu for extra checks and does not stop anything (**read** in the session; not a cause of the crash).

### Graphics backend (read: commit 7a47265)

The other project took the fix from `WINDOWS_GRAPHICS.md`: `wgpu_options()` in `crates/myapp/src/lib.rs` sets `setup.instance_descriptor.backends = wgpu::Backends::DX12` when `cfg!(windows)` and `WGPU_BACKEND` is unset. **Measured**: the owner reported the app runs on the Intel UHD laptop with it. The probe design in that note is not implemented there.

### Tested on the real laptop

- **Measured** (owner reported): the x86_64 installer installs and the app runs (2026-11-21); the installer closes a running app.
- **Not verified**: the Arm installer has never run on an Arm machine.

## Linux

### The tarball (read: `packaging/linux/bundle.sh`)

`myapp-linux-<arch>.tar.gz` holds:

```
myapp.app/
  bin/myapp
  lib/                      libraries the program needs
  share/applications/io.github.example.myapp.desktop
  share/icons/hicolor/512x512/apps/myapp.png
  share/icons/hicolor/1024x1024/apps/myapp.png
  install.sh  uninstall.sh  licenses.md
```

- Libraries: `ldd` on the program, then `cut -s -d' ' -f3`, then leave out glibc's own libraries (`libc`, `libm`, `librt`, `libutil`, `libnss_*`, `ld-linux`, …), `libasound`, and the GLib family. The program finds them through an rpath set only in the packaging build: `.cargo/bundle-config.toml` has `[target.'cfg(target_os = "linux")'] rustflags = ["-C", "link-args=-Wl,--disable-new-dtags,-rpath,$ORIGIN/../lib"]` (Zed's way). `bundle.sh` refuses to run when `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` is set, because either replaces those rustflags.
- **Measured**: a bundled `librt.so.1` failed on Arch with "`GLIBC_PRIVATE` not found"; leaving every glibc library out fixed it (commit 2f265d2).
- **Measured**: `ldd` on two files prints heading lines without a space, which `cut -d' ' -f3` passes through; release run 44931206245 failed on `cp: cannot stat '…libxkbcommon-x11.so.0:'`. `cut -s` fixed it.
- `libxkbcommon-x11`: winit opens it at run time only under X11, so `ldd` does not list it. `bundle.sh` finds it with `/sbin/ldconfig -p` and copies it in; the CI job installs `libxkbcommon-x11-0`. **Measured**: without it the program panicked under X11 with "Library libxkbcommon-x11.so could not be loaded" on the owner's Arch; `sudo pacman -S libxkbcommon-x11` worked around it before the library was shipped.
- Built in an `ubuntu:20.04` container so the program needs glibc 2.31 or later (commit 0471727).
- eframe features: the workspace eframe has default features off; `crates/myapp/Cargo.toml` adds `wayland` and `x11` under `[target.'cfg(target_os = "linux")'.dependencies]`.

### install.sh / uninstall.sh (read: `packaging/linux/`)

- `install.sh` copies the folder to `~/.local/myapp.app`, links `~/.local/bin/myapp`, and writes the desktop file to `~/.local/share/applications/` with `sed` turning `Exec=myapp` and `Icon=myapp` into absolute paths inside `~/.local/myapp.app`. It runs `update-desktop-database` when present.
- It runs `ldd` on the installed program and prints every `=> not found` library as a warning (Zed's check, commit b3d01f4).
- When `~/.local/bin` is not on `PATH`, it prints how to add it for zsh, fish and bash (Zed's text, commit c394e08).
- `uninstall.sh` removes the link, the desktop file, the cache and the app folder, and asks `[y/N]` about the settings and the user's documents, reading the answer from `/dev/tty`.
- Desktop file: `GenericName`, `Keywords` (commit 0cdc271) and `StartupWMClass=io.github.example.myapp`, matching `ViewportBuilder::with_app_id("io.github.example.myapp")` in `crates/myapp/src/lib.rs` (commit e12b0d5).

### Tested

- **Measured** (owner reported, 2026-11-21): on Arch under WSL2 with XFCE, the program runs from the terminal, but the XFCE menu entry opens no window. The log from the menu shows X11 (`DISPLAY=:10.0`, no `WAYLAND_DISPLAY`) and no panic. The terminal there has `WAYLAND_DISPLAY=wayland-0` (WSLg), so winit uses Wayland from the terminal and X11 from the menu.
- **Measured** (owner reported, 2026-11-21): on Arch installed on a real laptop with lightdm and XFCE, the tarball installs and the menu entry starts the app. The menu failure is specific to WSL.
- **Not verified**: the Arm tarball has never run on an Arm machine.
- Open point: started from the WSLg terminal (Wayland), the window has a bare frame with no title and tiny buttons (owner: "it does not look native", **measured**). WSLg draws no window frames, so winit draws its fallback. `cargo tree -e features -i winit` for `x86_64-unknown-linux-gnu` shows winit's `wayland` and `x11` on and no `sctk-adwaita` / `wayland-csd-adwaita` (**read**, 2026-11-24). Zed draws its own title bar on Linux (`window_decorations = "client"`). The owner has not chosen between the two.

## macOS

(read: `packaging/macos/bundle.sh`, `.cargo/config.toml`, `macos/Info.plist.ext`, `crates/myapp/Cargo.toml`)

- One `.dmg` per CPU, `MyApp-aarch64.dmg` and `MyApp-x86_64.dmg`, both built on `macos-latest` (commit 1d59391).
- `bundle.sh` builds with the bundle config, then runs Zed's cargo-bundle fork with `CARGO_BUNDLE_SKIP_BUILD=true`, `strip -x`, writes `licenses.md` into `Contents/Resources`, signs ad hoc (`codesign --force --deep -s -`), and makes a UDZO image with `hdiutil` holding the app and a `/Applications` link.
- Zed's fork is needed for `osx_info_plist_exts` in `[package.metadata.bundle]`; upstream cargo-bundle failed with "unknown field `osx_info_plist_exts`" (**measured**, run 44914287599). Installed in CI with `cargo install cargo-bundle --git https://github.com/zed-industries/cargo-bundle.git --rev 2be2669972dff3ddd4daf89a2cb29d2d06cad7c7 --locked`.
- `.cargo/config.toml` sets `[env] MACOSX_DEPLOYMENT_TARGET = "11.0"`, matching `osx_minimum_system_version = "11.0"`. rustc's own default is 11.0 for arm64 but 10.12 for x86_64, so without the variable the Intel build would claim older macOS than the bundle says (read in the session's Zed comparison, **not verified** by building without it).
- `macos/Info.plist.ext` adds `CFBundleSupportedPlatforms = [MacOSX]`, as Zed's Info.plist has (commit e2874e8).
- Signing: ad hoc only; no Developer ID, no notarization. Gatekeeper therefore blocks the first open of a downloaded copy. The other project's README and release carry no Gatekeeper note (read); ferd's `.github/release-notes.md` already has one.
- **Not verified**: neither `.dmg` from CI has been opened on another Mac by the owner.

## CI and releases

### `release.yml` (read)

| Job | Runner | Targets | Upload |
|---|---|---|---|
| `macos` | `macos-latest` | `aarch64-apple-darwin`, `x86_64-apple-darwin` | `MyApp-*.dmg` |
| `windows` | `windows-latest` | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | `MyApp-*.exe` |
| `linux` | `ubuntu-latest`, `ubuntu-24.04-arm`, in `ubuntu:20.04` | host | `myapp-linux-*.tar.gz` |
| `publish` | `ubuntu-latest` | — | `gh release create "$GITHUB_REF_NAME" --generate-notes …` |

- Starts on a `v*` tag push and on `workflow_dispatch`. `publish` runs only when the ref is a `v` tag (`if: startsWith(github.ref, 'refs/tags/v')`), so a run started by hand only builds and leaves the files on the run.
- Every upload has `if-no-files-found: error` (commit 7503f3b). Each matrix entry has its own `Swatinem/rust-cache` key. The toolchain comes from `rust-toolchain.toml` via `rustup toolchain install`.

### Test releases with a moved tag

- Test builds use a throwaway tag `v0.0.0-test`, force-moved to a new commit and pushed.
- **Measured** (2026-11-22, run 45030997863): all six build jobs passed; `publish` failed, because `gh release create` fails when the release `v0.0.0-test` already exists. GitHub kept serving the older files.
- The way around used: `gh run download 45030997863` into a folder (one subfolder per artifact), then serving those files to the test machines. Deleting the old release first (`gh release delete v0.0.0-test --yes --cleanup-tag`) before pushing the moved tag also avoids it (read in the session, **not verified** in a run).

Sizes from that run (measured):

| File | Bytes |
|---|---|
| `MyApp-x86_64.exe` | 8807498 |
| `MyApp-aarch64.exe` | 8301354 |
| `myapp-linux-x86_64.tar.gz` | 12751727 |
| `myapp-linux-aarch64.tar.gz` | 11925905 |
| `MyApp-x86_64.dmg` | 10480004 |
| `MyApp-aarch64.dmg` | 9501574 |

### `ci.yml` (read)

- `on: push` with no branch filter, plus `pull_request`; macOS, Ubuntu and Windows; `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`.
- **Measured**: a tag push also starts CI; the `v0.0.0-test` push of 2026-11-22 started CI run 45030997891 next to release run 45030997863. Whether to stop that is undecided.

## Testing from the Mac

(measured, 2026-11-18 to 2026-11-21)

- Windows exe cross-built with `cargo zigbuild --release -p myapp --target x86_64-pc-windows-gnu` (33 MB). zig supplies the C compiler and linker for crates with C code. `WINDOWS_GRAPHICS.md` notes that ferd links with Homebrew's `mingw-w64` instead and needs no zig.
- Linux program cross-built with cargo-zigbuild for `x86_64-unknown-linux-gnu`; it linked only after making `libdbus-sys` build its own D-Bus (specific to that project, and not in the repo).
- Files shared to the laptops with `python3 -m http.server 8000` in the build folder, at the Mac's Wi-Fi address; the address can change, and every link on the page carries it. Each address was fetched once before handing it over.
- A `commands.html` page in the same folder holds every command for the laptops in boxes with a copy button; plain `http://` blocks the Clipboard API, so the page falls back to `document.execCommand("copy")`. Windows commands are PowerShell; the Linux laptop's are fish.
- Limits: the cross-built files are bare programs, not the installer or tarball; the Inno installer builds only on Windows; the MSVC target, `crt-static` and the `VCRUNTIME140.dll` problem do not show in a `windows-gnu` build; a Linux program using the app's real link setup could not be linked on the Mac, so Linux-only code compiled and passed clippy but never ran before CI built it. After CI took over, every test used CI's files, downloaded and served the same way.

## Licenses

(read: `packaging/licenses/`, commit 2d4ad66)

- `packaging/licenses/generate.sh` writes `licenses.md` at packaging time; it is not in the repo. It installs `cargo-about@0.8.2` if missing and runs `cargo about generate --fail -c packaging/licenses/about.toml packaging/licenses/template.md.hbs`, treating any warning on stderr as a failure so no crate goes without its license, then turns HTML entities back into characters.
- `about.toml`: Zed's accepted list plus `OFL-1.1` and `Ubuntu-font-1.0`; `no-clearly-defined = true`; private crates ignored.
- Font and icon notices come first, copied from license files kept next to each font in the repo (`assets/fonts/*-LICENSE.txt`).
- Placement: `MyApp.app/Contents/Resources/licenses.md`, `myapp.app/licenses.md`, and the Windows install folder.
- Size (measured, 2026-11-22): 564 KB as text, about 31 KB compressed, 105 license texts.
- OFL reserved names: the OFL lets a font reserve its name for unmodified copies. Lato does, so the other project's cut-down `Lato-Regular.ttf` carries the internal name "MyApp Text" (commit b7b2d0d); `make_fonts.py` next to it rebuilds the file from upstream.

## Fonts and look

- egui's default font order (read, `epaint-0.37.0/src/text/font_definitions.rs`): `Proportional` is `Ubuntu-Light`, then the fallbacks (`egui-icons`, plus `NotoEmoji-Regular` and `emoji-icon-font` with the `monochrome_emoji_fonts` feature); `Monospace` is `Hack`, `Ubuntu-Light`, then the same fallbacks.
- Inserting a text font (read, `crates/myapp/src/theme.rs`): add it to `font_data`, then `families.entry(FontFamily::Proportional).or_default().insert(0, name)`. It becomes the first choice; Ubuntu Light and the emoji fonts stay behind it for missing characters.
- Pixel rounding: on a screen with one pixel per point, egui rounds glyph bounds to whole pixels, so two fonts with digits of equal height can line up a pixel apart. **Measured** on the Mac's 4K screen (2026-11-22): Barlow Condensed's and Ubuntu Light's digits measured the same height, and the gap came from rounding; on the iPad the gap matched the earlier look.

## What applies to ferd

Read from `feat/windows` at 239762b (`git ls-tree`, `git show`), compared with the other project:

- Releases: ferd's `release.yml` builds only macOS, as two zipped `Ferd.app` (no `.dmg`), on one `macos-latest` job. It has no Windows or Linux job, no `workflow_dispatch`, and the same `gh release create`, so a moved test tag hits the same publish failure.
- Windows: ferd has the windowed release exe (2ef7c17) and the DX12 default (494512f). It has no installer, no `.iss`, no single-instance mutex, no `build.rs` (no icon or version info in the exe), no `.cargo/config.toml` (no `crt-static`, so an MSVC build would need `VCRUNTIME140.dll`), and no Arm build.
- Linux: ferd has no tarball, no `install.sh` / `uninstall.sh`, no desktop file, no `bundle-config.toml` rpath. ferd's eframe uses default features, which already include `wayland` and `x11`. ferd's CI installs `libgtk-3-dev` for rfd, so `ldd` on a ferd program would list GTK libraries; whether to bundle them or leave them out like the GLib family is **not verified**.
- macOS: `bundle.sh` writes the Info.plist itself and signs ad hoc; it sets `LSMinimumSystemVersion` 11.0 but not `MACOSX_DEPLOYMENT_TARGET`, and has no `CFBundleSupportedPlatforms`. The Gatekeeper note in `.github/release-notes.md` is already there.
- Release settings: ferd's `[profile.release]` has only `strip = true`; no thin LTO, no `codegen-units = 1`, no size-optimized packaging profile.
- Licenses: ferd ships no `licenses.md` and has no cargo-about setup. It embeds no font files of its own (`canvas_fonts()` in `src/pen.rs` reuses egui's default fonts), so only crate licenses and egui's bundled fonts' licenses apply.
- CI: ferd's `ci.yml` runs on pushes to `main` and on pull requests only, so tag pushes do not start it.
