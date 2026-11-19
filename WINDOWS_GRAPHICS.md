# Windows graphics: the Intel Vulkan crash, the fix in place, and the design for choosing a backend

Temporary. Delete this file once the design below is implemented.

Each statement is marked by where it comes from: **measured** (seen on the Windows laptop or in a build), **read** (read in the source of eframe / egui-wgpu 0.37.0), **not verified** (reasoning or memory, untested).

## Current situation

- Branch `feat/windows` holds two code commits on top of `main`:
  - `90ad23f` build: hide the console window in Windows release builds
  - `93982a9` fix: use DX12 on Windows unless `WGPU_BACKEND` is set
- Measured: with `93982a9`, double-clicking `ferd.exe` on the laptop opens the app. Before it, ferd died at startup.
- On Windows ferd allows only DX12. A machine where DX12 fails gets no fallback, and the windowed exe closes without any message.
- The backend order, the probe process and the message box described under "The design" are not implemented.
- Nothing was changed for macOS or Linux: they use eframe's defaults.
- OpenTurbo (`/Users/scorphus/Workspace/OpenTurbo`) starts the same way and has no DX12 default. The same notes are in its `WINDOWS_GRAPHICS.md`.

## The problem

On Pablo's Windows laptop (fresh Windows install, Intel UHD graphics), ferd built with eframe's default backend choice dies at startup. No window, no message.

Measured, from the Windows Application event log (`Application Error` events):

```
Faulting application name: ferd.exe
Faulting module name: igvk64.dll, version: 31.0.101.2130, time stamp: 0x66bb38c5
Exception code: 0xc0000005
Fault offset: 0x000000000064ea72      (two crashes: ferd.exe, ferd-debug.exe)
Fault offset: 0x0000000000630e61      (one crash: ferd.exe)
Faulting module path: C:\WINDOWS\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_c3bafbe80a0ff678\igvk64.dll
```

- `igvk64.dll` is Intel's Vulkan driver. `0xc0000005` is an access violation: Windows kills the process, no Rust code runs afterwards, nothing can be caught in-process.
- Measured: the crash comes and goes with the same exe. `ferd.exe` started from PowerShell with `.\ferd.exe` stayed open at least once; the same file double-clicked in Explorer started (visible in `Get-Process ferd`) and was gone about a second later. Both the windowed release exe and the console exe crashed.
- Not known: at which step the driver crashes (listing adapters, creating the device, creating the swapchain, or drawing the first frame). Two different fault offsets were logged.
- The driver is Intel's 31.0.101.2130, released 2024-09-03 for 7th to 10th gen processors, a branch Intel maintains for security fixes only. A forum mirror lists a newer 31.0.101.2135 (2025-03-28); not confirmed on Intel's page. A driver update was not tried.

This is not an egui or Rust problem. The same `igvk64.dll` access violation at startup is reported for Quake, Dolphin, Minecraft and Unity builds:

- https://community.intel.com/t5/Graphics/Intel-Arc-VGA-k-vulkan-support/m-p/1756916
- https://community.intel.com/t5/Graphics/Dolphin-Emulator-Vulkan-Crash/m-p/578435
- https://mojira.dev/MC-307412
- https://issuetracker-mig.prd.it.unity3d.com/issues/unity-vulkan-build-crashes-when-changing-window-size-on-an-intel-gpu

Three web searches found no report with our exact combination (wgpu, `igvk64.dll`, startup crash, Intel UHD). Closest wgpu-side material:

- https://github.com/gfx-rs/wgpu/issues/4599 (rendering bug on Intel UHD Vulkan, not a crash)
- https://gitlab.com/veloren/veloren/-/issues/1177 (a wgpu game working around a Vulkan problem by forcing another backend with `WGPU_BACKEND`)
- https://wgpu.rs/doc/src/wgpu/documentation/internals/known_driver_issues.rs.html
- https://github.com/emilk/egui/issues/4885 (the opposite case: DX12 failing in eframe on someone's machine while Vulkan and GL work, so DX12-only is not safe everywhere)

## Why the default reaches the Vulkan driver (read)

ferd depends on `eframe = "0.37.0"` with default features. Those turn on the `wgpu` feature, not glow (`cargo tree -e features -i eframe`).

`egui-wgpu-0.37.0/src/setup.rs`, `WgpuSetupCreateNew::without_display_handle()`:

```rust
backends: wgpu::Backends::from_env()
    .unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL),
```

So with `WGPU_BACKEND` unset, Vulkan, DX12 and GL are all enabled on Windows.

`egui-wgpu-0.37.0/src/lib.rs`, `RenderState::create`:

```rust
let backends = if let WgpuSetup::CreateNew(create_new) = &config.wgpu_setup {
    create_new.instance_descriptor.backends
} else {
    wgpu::Backends::all()
};
instance.enumerate_adapters(backends).await
```

eframe lists the adapters of every enabled backend before it picks one. So the Vulkan driver is loaded whenever Vulkan is in `instance_descriptor.backends`, even if another backend would be picked. The only way to keep `igvk64.dll` out of the process is to leave Vulkan out of `backends`.

## What was tried, and the result

| Tried | Result |
| --- | --- |
| `ferd.exe` with eframe's default backends, double-clicked | measured: crash in `igvk64.dll`, three logged |
| The same `ferd.exe` from PowerShell, `.\ferd.exe` | measured: opened at least once |
| `$env:RUST_LOG="debug"; $env:RUST_BACKTRACE=1; .\ferd-debug.exe` | measured: opened once, crashed once (22:44 event). ferd reads neither variable, and neither shows anything for this crash |
| `$env:WGPU_BACKEND="dx12"; .\ferd.exe` | handed to Pablo; no result reported. Worked for OpenTurbo on the same laptop, per Pablo |
| DX12 as the Windows default in code (`93982a9`) | measured: double-click opens the app |
| `DX12 \| GL` instead of DX12 only | not tried |
| Updating the Intel driver | not tried |

## What works today

### 1. DX12 as the Windows default (`93982a9`)

```rust
// in main(), after building `options`
#[cfg(windows)]
let options = dx12_unless_chosen(options);

// Intel's Vulkan driver crashes while wgpu sets up the device on some laptops.
#[cfg(windows)]
fn dx12_unless_chosen(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if std::env::var_os("WGPU_BACKEND").is_none()
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
    {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
    }
    options
}
```

A cheap improvement, not tested: `eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL`. A machine with no DX12 adapter would fall back to OpenGL and the Vulkan driver would still never load.

### 2. Windowed release exe, console debug exe (`90ad23f`)

```rust
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
```

Measured: PE header subsystem 2 (windowed) for the release exe, 3 (console) for the exe built with debug assertions. A windowed exe prints nothing anywhere, so a crash or error is invisible unless it is shown in a message box or read from the event log.

### 3. Other Windows checks (read in ferd's source, no change needed)

- The user name falls back from `USER` to `USERNAME` (`src/share.rs`).
- The `.ferd` layout file sits next to the source file; no path is built from `HOME`.
- The file watcher watches the source file's folder; the path is canonicalized first.
- Both Windows builds raise no warnings.

## The design (not implemented)

Goal: pick the graphics backend the way game engines do. Have a preference list per OS, try each entry for real, move to the next one when it fails, and show a clear message when none works. It would apply to the Windows, Linux and macOS builds.

1. **Preference order.** Windows: DX12, Vulkan, GL. Linux: Vulkan, GL. macOS: Metal only. `WGPU_BACKEND`, when set, is the whole list, with no fallback.
2. **Each backend is tried in a throwaway process.** ferd starts itself with a hidden argument (for example `ferd --probe vulkan`); no second binary. The main process reads the exit status: 0 means the backend works, anything else (an error, or the process being killed by Windows for a driver crash) means try the next one. This is what makes a driver access violation survivable: only the probe dies.
3. **The probe goes as far as needed to show ferd can really run on that backend.** It runs the same eframe startup as the real app, on that one backend, up to the first frame shown, and exits 0 only after that. Device creation alone is not enough, because where the Intel driver crashes is not known.
4. **The main process then starts eframe with only the winning backend** in `instance_descriptor.backends`, so no other driver is loaded.
5. **When every backend fails**, show a native message box (`rfd::MessageDialog`; rfd is already a dependency) listing each backend tried and why it failed, and print the same to stderr. The probe's printed error text is the "why".
6. **Integrated GPU first**, because ferd draws boxes: `power_preference` current `HighPerformance` (eframe's default) would become `LowPower`, with `WGPU_POWER_PREF` still winning. Proposed by Claude; Pablo has not confirmed this item.

Limits and open points:

- A probe that passes proves the backend worked once in that process. The Intel crash came and went with the same exe, so the main process can still crash on a backend that passed. The order (DX12 first on Windows) is what protects that laptop.
- Every start pays for one extra process and one driver load for the first backend. Not measured; a guess is 100 to 300 ms.
- Whether eframe draws a frame to an invisible window (`ViewportBuilder::with_visible(false)`) is not tested. If it does not, the probe needs another way to reach the first frame.
- macOS has one backend, so the probe can be skipped there.
- On macOS, `macos::listen()` runs at the top of `main`; the probe argument would have to be handled so a probe never takes a file sent by Finder. Not looked into.

Ideas considered and dropped:

- **In-process try and fall back only** (catch the error from wgpu, try the next backend): handles errors, cannot handle the access violation.
- **Retry loop around `eframe::run_native`**: winit cannot create its event loop twice in one process (not verified, from memory).
- **Create the device ourselves and hand it over with `WgpuSetup::Existing`**: ferd would have to own the event loop (`eframe::create_native` exists for that) and pass the Wayland display handle itself. More code than letting eframe create the device on the chosen backend.
- **A per-user file** written before a backend is used and deleted after the first frame, so the next start skips a backend that crashed: it would be ferd's first per-user state file and costs the user one crashed start. Replaced by the probe process.
- **`native_adapter_selector`** (read: a hook in `WgpuSetupCreateNew` that receives every adapter and returns the one to use): useful for choosing between GPUs, but it runs after the adapters are listed, so the Vulkan driver is already loaded.

Read in the source, useful when implementing:

- Startup failures come back from `eframe::run_native` as `Err(eframe::Error::Wgpu(..))` (`eframe-0.37.0/src/lib.rs`; `src/native/wgpu_integration.rs` uses `?` on `painter.set_window`).
- `eframe::create_native` (`eframe-0.37.0/src/lib.rs`) takes an event loop the app owns.
- On `x86_64-pc-windows-gnu`, wgpu is built with the `dx12`, `gles` and `metal` features on (`cargo tree -e features -i wgpu`).

## Building and testing for Windows from the Mac (measured)

- `brew install mingw-w64` and `rustup +1.98.1 target add x86_64-pc-windows-gnu`, then:

  ```sh
  cargo +1.98.1 build --release --target x86_64-pc-windows-gnu
  ```

  rustc finds `x86_64-w64-mingw32-gcc` as the linker by itself. `cargo-zigbuild` is not needed. The `stable` toolchain on the Mac is 1.91.1, too old for eframe 0.37, hence `+1.98.1`.
- The plain debug build fails to link: GNU ld stops on iroh's debug DLL with `export ordinal too large: 109764`. A console exe with debug info that does link:

  ```sh
  CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=1 \
    cargo +1.98.1 build --release --target x86_64-pc-windows-gnu --target-dir target/win-debug
  ```

  Debug assertions on means the `windows_subsystem` line does not apply, so this exe has a console. It is copied to `target/x86_64-pc-windows-gnu/release/ferd-debug.exe`.
- Both exes and a `commands.html` page with a copy button per command are served to the laptop with:

  ```sh
  python3 -m http.server 8000 -d target/x86_64-pc-windows-gnu/release
  ```

- PowerShell on the laptop:

  ```powershell
  # is it running right now
  Get-Process ferd -ErrorAction SilentlyContinue | Select-Object Id, Path, MainWindowTitle
  # exit code of a windowed exe
  $p = Start-Process .\ferd.exe -Wait -PassThru; '{0:X}' -f $p.ExitCode
  # last three hard crashes, with the faulting module
  Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Application Error'} -MaxEvents 3 | Format-List TimeCreated, Message
  ```

  The event log is what named `igvk64.dll`.
