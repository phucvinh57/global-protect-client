# Repository Guidelines

## Project Structure & Module Organization

This is a Tauri 2 desktop application with a React 19 and TypeScript frontend. Frontend code lives in `src/`: use `hooks/` for hooks, `providers/` for context, `utils/` for helpers, `styles/` for global CSS and tokens, and `assets/` for imported files. Files copied directly by Vite belong in `public/`. Rust entry points, Tauri configuration, capabilities, and icons live in `src-tauri/`. Generated output (`dist/`, `src-tauri/target/`) must not be committed.

## Build, Test, and Development Commands

- `bun install` installs the locked JavaScript dependencies from `bun.lock`.
- `bun run dev` starts the frontend-only Vite server on port 1420.
- `bun run tauri dev` launches the desktop app with hot reload.
- `bun run build` type-checks TypeScript and creates the frontend production bundle.
- `bun run tauri build` builds distributable native packages.
- `bunx biome check .` runs the configured formatter and recommended lint rules; add `--write` to apply safe fixes.
- `cargo test --manifest-path src-tauri/Cargo.toml` compiles and runs Rust tests.

## Coding Style & Naming Conventions

TypeScript is strict; keep imports organized and use the `@/` alias for modules under `src`. Biome is authoritative for TS/TSX formatting: tabs, double quotes, and its recommended lint preset. Name React components and providers in PascalCase, hooks as `useSomething`, and source files in kebab-case (for example, `theme-provider.tsx`). Keep Rust code compatible with `cargo fmt`; use `snake_case` for functions and Tauri commands. Prefer small, typed utilities over duplicated inline logic.

## Testing Guidelines

No frontend test framework or coverage threshold is configured yet. For every change, run `bun run build` and manually exercise the affected flow in `bun run tauri dev`. Place future frontend tests beside their subject as `*.test.ts` or `*.test.tsx`; place Rust unit tests in a `#[cfg(test)]` module. Add a documented test script when introducing a frontend runner.

## Security & Configuration

Do not commit credentials, VPN configuration, certificates, or machine-specific `.local` files. Tauri's CSP is currently disabled; treat any new remote content or capability as security-sensitive and document the rationale in the pull request.
