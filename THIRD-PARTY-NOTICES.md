# Third-party notices

Monitor Brightness Control bundles and links the following third-party components. Their original copyright notices and license texts are reproduced (or referenced) below. The first-party code in this repository is proprietary; see [`LICENSE`](LICENSE). Bundling these MIT- and Apache-2.0-licensed libraries inside a proprietary product is permitted by their respective licenses, provided their copyright and permission notices are included — that is what this file does.

This list covers direct dependencies. The full transitive list — kept current by `cargo` and `npm` — can be regenerated with:

```bash
cargo install cargo-about
cargo about generate about.hbs > THIRD-PARTY-NOTICES.generated.md

cd app
npx license-checker-rseidelsohn --production --summary
```

Run those before each release; the file below is a curated quick reference, not an exhaustive audit.

---

## Tauri framework and plugins

* [`tauri`](https://github.com/tauri-apps/tauri) — © Tauri Programme within The Commons Conservancy. **MIT OR Apache-2.0**.
* `tauri-build`, `tauri-plugin-global-shortcut`, `tauri-plugin-single-instance`, `tauri-plugin-autostart`, `tauri-plugin-window-state`, `tauri-plugin-os`, `tauri-plugin-shell`, `tauri-plugin-dialog` — all © Tauri Programme. **MIT OR Apache-2.0**.
* [`@tauri-apps/api`](https://github.com/tauri-apps/tauri/tree/dev/packages/api) — © Tauri Programme. **MIT OR Apache-2.0**.
* `@tauri-apps/plugin-autostart`, `@tauri-apps/plugin-os` — © Tauri Programme. **MIT OR Apache-2.0**.

## Rust runtime crates

| Crate | Authors / Project | License |
|---|---|---|
| `tokio` | Tokio Contributors | MIT |
| `serde`, `serde_json` | David Tolnay et al. | MIT OR Apache-2.0 |
| `parking_lot` | Amanieu d'Antras | MIT OR Apache-2.0 |
| `once_cell` | Aleksey Kladov | MIT OR Apache-2.0 |
| `bitflags` | The Rust Project Developers | MIT OR Apache-2.0 |
| `thiserror`, `anyhow` | David Tolnay | MIT OR Apache-2.0 |
| `log`, `env_logger` | The Rust Project Developers | MIT OR Apache-2.0 |
| `chrono` | Kang Seonghoon et al. | MIT OR Apache-2.0 |
| `sunrise` | Nathan Reed | MIT |
| `toml` | Alex Crichton | MIT OR Apache-2.0 |
| `dirs` | Simon Ochsenreither | MIT OR Apache-2.0 |
| `clap` | The Clap Authors | MIT OR Apache-2.0 |

## Platform crates

| Crate | Platform | Authors | License |
|---|---|---|---|
| `windows` | Windows | Microsoft | MIT OR Apache-2.0 |
| `wmi` | Windows | Lior Ramati | MIT |
| `core-foundation` | macOS | Servo Project | MIT OR Apache-2.0 |
| `udev` | Linux | Christian Loehnert, contributors | MIT |

## Frontend (npm)

| Package | Authors | License |
|---|---|---|
| `vite` | Yuxi (Evan) You & contributors | MIT |
| `typescript` | Microsoft | Apache-2.0 |
| `esbuild` | Evan Wallace | MIT |
| `rollup`, `picocolors`, `source-map-js`, `postcss`, `nanoid` | various | MIT |

---

## Full license texts

The license texts are reproduced from the upstream projects. Where a crate is dual-licensed, only the MIT text is reproduced below — that is the license under which the code is *used* by this project, and the bundling permission applies under either prong of the dual license.

### MIT License (Tauri, serde, tokio, and many others)

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

### Apache License 2.0 (TypeScript and the Apache prong of dual-licensed Rust crates)

Full text at <https://www.apache.org/licenses/LICENSE-2.0>. The Apache-2.0 license adds an explicit patent grant and a NOTICE-passthrough requirement; no NOTICE files are produced by the dependencies listed here.

---

## Reporting a missing notice

If you spot a dependency that is bundled in the shipped binary but not listed here, please open an issue. We will add it in the next release.
