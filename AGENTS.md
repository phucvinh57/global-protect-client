# Repository Guidelines

## Project Structure & Module Organization

This is a Tauri 2 desktop application with a React 19 and TypeScript frontend. Frontend code lives in `src/`: use `hooks/` for hooks, `providers/` for context, `utils/` for helpers, `styles/` for global CSS and tokens, and `assets/` for imported files. Files copied directly by Vite belong in `public/`. Rust entry points, Tauri configuration, capabilities, and icons live in `src-tauri/`. Generated output (`dist/`, `src-tauri/target/`) must not be committed.

The Rust workspace under `src-tauri/crates/` splits the VPN work in two. `gp-auth` speaks the GlobalProtect portal and gateway HTTP protocol itself and returns a session cookie; libopenconnect's own GlobalProtect login cannot send the client identity fields modern PAN-OS deployments require and is answered with HTTP 512. `openconnect`/`openconnect-sys` wrap libopenconnect for the tunnel only, driven by that cookie. `gp-helper` is the privileged process the GUI talks to over JSON lines.

`src-tauri/src` holds the window's own surface: `settings.rs` stores the list of saved connection profiles in `settings.json` (never passwords), `credentials.rs` puts passwords in the keyring, `helper_process.rs` owns the helper and the single `VpnRuntime` that both the window and the tray read, and `tray.rs` keeps the tray icon and menu in step with that state. Only one tunnel exists at a time, so the runtime holds one active profile. Closing the window hides it; only the tray's Quit ends the process. The helper's log stream is diagnostic and goes to stderr — the window is told about failures through `vpn://error` alone, and shows no log panel.

## Build, Test, and Development Commands

- `bun install` installs the locked JavaScript dependencies from `bun.lock`.
- `bun run dev` starts the frontend-only Vite server on port 1420.
- `bun run tauri dev` launches the desktop app with hot reload.
- `bun run build` type-checks TypeScript and creates the frontend production bundle.
- `bun run dev` then `http://localhost:1420/index.mock.html` renders the window against a stubbed Tauri bridge, so the UI can be reviewed without a portal or root. Query parameters drive it: `state=connected|connecting`, `empty=1`, `theme=dark`, `open=<button label>`. Vite builds only `index.html`, so the harness never ships.
- `bun run tauri build` builds distributable native packages (`deb` and `rpm`).
- `bun run scripts/build-helper.ts [--release] [--target <triple>]` builds `gp-helper` and stages it as `src-tauri/binaries/gp-helper-<triple>`. Both `dev:tauri` and `build:tauri` call it, and Tauri's `bundle.externalBin` only ships a sidecar staged under that exact per-triple name; `tauri-build` then copies it next to `gp-client` so `helper_path()` resolves in dev and after install alike. Building the helper with a bare `cargo build` leaves the packaged app spawning a `gp-helper` that was never installed.
- `bunx biome check .` runs the configured formatter and recommended lint rules; add `--write` to apply safe fixes.
- `cargo test --manifest-path src-tauri/Cargo.toml --workspace` compiles and runs Rust tests; without `--workspace` the crate tests under `src-tauri/crates/` are skipped.
- `cargo run -p gp-auth --example probe -- https://<portal> <user> [linux|windows|mac]` exercises the login against a real portal without root, a tunnel or the GUI, printing every request and the server's exact reply (including the `x-private-pan-globalprotect` header that explains an HTTP 512).

## Coding Style & Naming Conventions

TypeScript is strict; keep imports organized and use the `@/` alias for modules under `src`. Biome is authoritative for TS/TSX formatting: tabs, double quotes, and its recommended lint preset. Name React components and providers in PascalCase, hooks as `useSomething`, and source files in kebab-case (for example, `theme-provider.tsx`). Keep Rust code compatible with `cargo fmt`; use `snake_case` for functions and Tauri commands. Prefer small, typed utilities over duplicated inline logic.

## Testing Guidelines

No frontend test framework or coverage threshold is configured yet. For every change, run `bun run build` and manually exercise the affected flow in `bun run tauri dev`. Place future frontend tests beside their subject as `*.test.ts` or `*.test.tsx`; place Rust unit tests in a `#[cfg(test)]` module. Add a documented test script when introducing a frontend runner.

## Security & Configuration

Do not commit credentials, VPN configuration, certificates, or machine-specific `.local` files. Tauri's CSP is currently disabled; treat any new remote content or capability as security-sensitive and document the rationale in the pull request. Passwords belong in the keyring by way of `credentials.rs` and must never reach `settings.json`; the window is only ever told whether a password exists, never what it is.
