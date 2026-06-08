# Building Monitor Brightness Control

## Toolchain prerequisites

| | Windows | macOS | Linux |
|-|-|-|-|
| Rust | 1.77+ (`stable-x86_64-pc-windows-msvc`) | 1.77+ (universal toolchain) | 1.77+ |
| Node.js | 20 LTS | 20 LTS | 20 LTS |
| Tauri CLI | `cargo install tauri-cli --locked` | same | same |
| Extras | Visual Studio 2022 Build Tools (C++) + Windows 10/11 SDK | Xcode 15 command-line tools | `libudev-dev pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libi2c-dev` |

## CLI only

```bash
cargo build --release -p brightness-cli
./target/release/mbc list
./target/release/mbc benchmark
```

## Full app

```bash
cd app
npm install            # one-time per checkout
npm run build          # produces app/dist/ (Tauri reads from here in release)
cd ..
cargo build --release -p monitor-brightness-control --features custom-protocol
```

A workspace alias wraps that exact command, so you can also run:

```bash
npm --prefix app run build    # refresh app/dist/
cargo app-release             # = build --release -p monitor-brightness-control --features custom-protocol
```

`--features custom-protocol` is **required** when building with plain `cargo build`. Without it, Tauri's webview points at `devUrl` (`http://localhost:5173`) and the running app shows `ERR_CONNECTION_REFUSED`. To make this mistake impossible to ship, the app crate carries a `compile_error!` guard that fails any release build that omits the feature. The wrapper `cargo tauri build` enables this feature automatically:

```bash
cargo install tauri-cli --version "^2.0" --locked
cargo tauri build      # one-shot: builds frontend + backend with the right features
```

`tauri build` also runs `npm run build` for you via `beforeBuildCommand`.

## Production bundles

### Windows

NSIS installer (`.exe`) and MSI (`.msi`) are produced by `cargo tauri build`:

```powershell
cd app/src-tauri
cargo tauri build --target x86_64-pc-windows-msvc
```

For Microsoft Store packaging, the MSIX layout uses [`packaging/windows/AppxManifest.xml`](../packaging/windows/AppxManifest.xml); the Store submission workflow itself is a maintainer-only document.

### macOS

```bash
cd app/src-tauri
cargo tauri build --target universal-apple-darwin
```

For notarization the following environment variables must be set:

* `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` — the Developer ID Application `.p12`.
* `APPLE_SIGNING_IDENTITY` — `"Developer ID Application: NAME (TEAM_ID)"`.
* `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` — for `notarytool`.

[`packaging/macos/entitlements.plist`](../packaging/macos/entitlements.plist) is referenced by `tauri.conf.json`.

### Linux

```bash
cd app/src-tauri
cargo tauri build --target x86_64-unknown-linux-gnu
```

This produces:
* `target/release/bundle/deb/*.deb`
* `target/release/bundle/appimage/*.AppImage`
* `target/release/bundle/rpm/*.rpm` (if `rpmbuild` is installed)

Flatpak builds use a separate manifest:

```bash
flatpak-builder --user --install --force-clean build-flatpak \
    packaging/linux/io.github.monitorbrightnesscontrol.app.yml
```

To use the app without root, install the udev rule once:

```bash
sudo install -m 0644 packaging/linux/90-monitor-brightness.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
sudo usermod -aG i2c,video "$USER"
# log out and back in for groups to take effect
```

## Releasing

The project version has a **single source of truth**: `[workspace.package].version`
in the root [`Cargo.toml`](../Cargo.toml). `tauri.conf.json` has no `version` key,
so Tauri derives it from the crate (`CARGO_PKG_VERSION`) — that is what the
installers, the bundle metadata and the in-app About screen (`getVersion()`) all
report. Never hardcode a version anywhere else.

To cut a release, bump in that one place with the helper, which also syncs the
non-derivable copies (`Cargo.lock`, `app/package.json`, `app/package-lock.json`):

```bash
scripts/bump-version.sh 1.2.3
git commit -am "release: v1.2.3"
git tag -a v1.2.3 -m "release: v1.2.3"
git push origin main && git push origin v1.2.3   # the tag triggers release.yml
```

## Continuous integration

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs `fmt --check`, `clippy`, `cargo test`, the frontend build and a Tauri bundle on Windows / macOS / Linux for every push.

[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds and uploads signed bundles when a `v*.*.*` tag is pushed.

## Cross-checking platform code from a single host

The build host can `cargo check --target=x86_64-apple-darwin` etc. to catch syntax errors in foreign-platform modules. With `rustup`:

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu
cargo check --target x86_64-apple-darwin -p brightness-core
cargo check --target x86_64-unknown-linux-gnu -p brightness-core
```

(Linker errors are expected — only the type-check passes are useful here.)
