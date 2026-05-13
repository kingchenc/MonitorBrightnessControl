# Contributing

Thanks for considering a contribution. A few things worth knowing before you open a PR.

## License model

This project is **source-available, proprietary**. See [`LICENSE`](LICENSE). The repository is on GitHub so you can read it, run the official binaries, file issues and send pull requests — but forking it as an independent product, redistributing modified copies, or shipping it as a paid offering is not permitted.

By submitting a pull request you:

* Keep the copyright in your contribution.
* Grant the project owner (kingchenc) a **perpetual, worldwide, royalty-free, irrevocable, sublicensable license** to use, reproduce, modify, publish and distribute your contribution as part of the Software under this License or any future license the project owner chooses.

This is the standard "inbound = outbound" pattern adapted to a proprietary outbound license — it lets the project ship your fix without legal ambiguity, without taking ownership of your code.

If that is not acceptable to you, please don't open a PR — open an issue instead and describe what you would like to see fixed.

## What's welcome

* **Bug reports.** Include OS version, monitor make/model (and DDC/CI capability if known), reproduction steps, and the relevant log lines (run the app from a terminal to see them).
* **Platform fixes.** The macOS `IOAVService` path in particular needs hardware-on-hand testing; if you have an Apple Silicon Mac with an external display and the patience to test, PRs there are gold.
* **Additional translations.** Add a new locale by:
  1. Appending an entry to `app/src/i18n.ts` → `SUPPORTED_LOCALES`.
  2. Copying the English key block and translating each value.
  3. Updating the `Locale` type union at the top of `i18n.ts`.
* **Performance fixes**, especially in the DDC/CI hot paths.
* **Documentation improvements** — the `docs/` folder, the README, this file.

## What's not welcome

* **Feature creep.** New tabs, new dependencies, new background services — please open an issue first to discuss. Small, focused PRs merge fastest.
* **Style-only churn.** Reformatting whole files, renaming variables for taste, etc.
* **Mass dependency updates.** One PR per dependency bump, with the reason in the description.

## Development setup

```bash
git clone https://github.com/kingchenc/MonitorBrightnessControl
cd MonitorBrightnessControl

# Rust toolchain pinned by rust-toolchain.toml; rustup will install it.
cd app && npm install && cd ..

# Run the dev build (frontend + backend hot-reload)
cd app && npx tauri dev
```

For CLI-only changes you can skip the npm step entirely:

```bash
cargo run -p brightness-cli -- list
```

## Code style

* `cargo fmt` before pushing (run from the workspace root).
* `cargo clippy --workspace --all-targets -- -D warnings` should pass.
* Frontend: `npx tsc --noEmit` should pass. No specific Prettier config — keep it consistent with the surrounding file.

## Tests

* `cargo test --workspace` — unit and integration tests across all crates.
* CLI integration tests live in `crates/brightness-cli/tests/cli.rs`.

There is no UI test harness right now; manual testing in `tauri dev` is the rule for frontend changes.

## Commit messages

Imperative present tense, scoped prefix where it helps. Examples from the existing log:

```
feat: real EDID monitor names, autostart toggle, six-language UI
fix: tray duplicate, autodim runtime, custom-protocol release flag
docs+ci: packaging, store submission, CI workflows, integration tests
```

That's it. Thanks again.
