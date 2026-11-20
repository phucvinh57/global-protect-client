# gp-client

A GlobalProtect VPN client for Linux, with a Tauri desktop application and a
standalone Rust CLI. Both applications share profiles and system-keyring passwords.

```sh
gp-client profile add Work --portal https://vpn.example.com --username alice
gp-client connect Work
gp-client status
gp-client disconnect
```

`connect` prompts for the password and, when required, a one-time passcode,
certificate approval, MFA responses, and a gateway choice. Passwords and passcodes
are hidden and cannot be supplied through command-line arguments. It returns
success only when the tunnel is connected. The unprivileged background controller
then keeps the tunnel running after the terminal closes. `disconnect` waits for
the helper to exit and finish route/DNS cleanup; repeating it succeeds.

Profiles can be selected by ID or by a unique, case-sensitive name. Ambiguous names
report the matching IDs:

```sh
gp-client profile list
gp-client profile show Work
gp-client profile edit Work --name Office --client-os windows --hip
gp-client profile delete Office
```

`profile add` and `profile edit` support `--name`, `--portal`, `--username`,
`--remember`, `--cafile`, `--client-cert`, `--client-key`,
`--trusted-fingerprint`, `--gateway`, `--client-os linux|windows|mac`, `--script`,
`--hip`, and `--requires-otp`. Boolean options accept `=false` to disable them;
an empty string clears an optional text field, for example `--gateway ''`.
Missing portal and username fields are prompted for interactively. Enabling
password remembering requires confirmation, and each new password is stored only
after explicit consent. Without a working system keyring the connection still
works, but the password must be entered again next time.

With redirected input, commands fail clearly when a credential or answer is
missing. A remembered keyring password and cached sudo authorization allow a
noninteractive connection when no further questions are required. Certificate
approval requires a terminal unless the profile already has the trusted
fingerprint or the certificate validates normally.

## Standalone build and installation

The CLI does not depend on Tauri, GTK, or a webview. Install Rust, a C compiler,
Clang/libclang, pkg-config, and OpenConnect development files. On Debian/Ubuntu:

```sh
sudo apt install build-essential clang libclang-dev pkg-config libopenconnect-dev vpnc-scripts
./scripts/build-cli.sh --release
```

Always use a separate `--target-dir` for the CLI: its binary and the desktop
binary are both named `gp-client`. The script accepts Cargo flags such as
`--release` and `--target <triple>` and always selects `src-tauri/target/cli`. The host build above produces
`src-tauri/target/cli/release/gp-client` and `gp-helper`. This build does not run
the desktop build script or stage its sidecar.

Install the CLI separately from a desktop package:

```sh
sudo install -Dm755 src-tauri/target/cli/release/gp-client /usr/local/bin/gp-client
sudo install -Dm755 src-tauri/target/cli/release/gp-helper /usr/local/libexec/gp-client/gp-helper
```

Install `sudo`, `vpnc-scripts`, and the distribution's OpenConnect runtime library
on the target machine. Both installed files should be root-owned. The CLI asks
sudo to authenticate in the initiating terminal before it detaches, then uses
`sudo -n` to start only the helper as root. No setuid executable or passwordless
sudo rule is required. The controller remains your normal user. Helper lookup
checks `GP_HELPER_PATH`, an adjacent `gp-helper`, and finally
`/usr/local/libexec/gp-client/gp-helper`.

If the desktop package is installed at `/usr/bin/gp-client`, the usual PATH order
selects `/usr/local/bin/gp-client` first. Use the absolute executable path to
choose either application, or install the CLI as `/usr/local/bin/gp-client-cli`.
Installing the CLI does not overwrite the desktop package's files or helper.
Upgrade both helpers to obtain GUI/CLI tunnel locking; an older helper cannot
participate in the new lock.

## Shared data and session ownership

Both applications use
`${XDG_CONFIG_HOME:-$HOME/.config}/com.npvinh.gp-client/gp-client.db` and the
`com.npvinh.gp-client` keyring service. Passwords never enter SQLite. Released
migration SQL, descriptions, versions, and checksums are shared unchanged, so
either application can initialize or upgrade the store. A desktop window sees
changes through its existing profile-loading flow.

The controller uses an owner-checked private directory under `$XDG_RUNTIME_DIR`
(or `/tmp/gp-client-<uid>`), a mode-0600 Unix socket, and a lifetime file lock.
Runtime files contain no credentials. A stale socket can be removed only after
the controller lock is acquired. Authentication is cancelled if the initiating
terminal disappears before connection succeeds. After success the session lasts
until disconnect, tunnel failure, or shutdown; there is no automatic reconnect
or reboot persistence.

The privileged helper holds `/run/lock/gp-client-tunnel.lock` throughout login,
tunnel operation, and cleanup. GUI and CLI sessions remain independently
controlled: the CLI reports another owner and cannot disconnect a GUI session
(or another user's CLI session). New competing attempts fail without disturbing
the existing tunnel.

## Development and validation

```sh
bun install --frozen-lockfile
bun run build
bun run tauri dev
# This stages the per-target helper required by desktop builds/tests:
bun run scripts/build-helper.ts
cargo test --manifest-path src-tauri/Cargo.toml --workspace
# CLI-only checks/tests, including Python 3 stdlib simulated-helper and PTY tests:
cargo test --manifest-path src-tauri/Cargo.toml \
  -p gp-cli -p gp-store --target-dir src-tauri/target/cli
cargo clippy --manifest-path src-tauri/Cargo.toml \
  -p gp-cli -p gp-store --all-targets --target-dir src-tauri/target/cli -- -D warnings
```

The acceptance tests isolate config/runtime directories and the session bus, and
replace sudo/helper processes with test fixtures; they never change networking.
They cover profile options/CRUD, ambiguity, unavailable keyring, hidden terminal
input, MFA/certificate/gateway questions, cancellation, helper failures,
background lifetime, cleanup acknowledgment, stale sockets, and startup races.
Shared-store tests pin the released migration checksums and exercise upgrades.

For live validation, connect to an authorized test VPN, close the terminal, check
status from a new terminal, then disconnect and verify routes and DNS are
restored. Also check desktop profile load/edit/delete and GUI/CLI ownership
contention using the rebuilt helper. Live checks require a test portal and VPN
credentials.

## Releases

GitHub Actions' **Build release artifacts** workflow builds on Ubuntu 24.04,
checks the frontend and Rust workspace, and produces the desktop `.deb`, a
separate standalone CLI archive, and `SHA256SUMS`. Its default run only uploads
artifacts for review. Select **Publish the built version as a GitHub release**
to publish after the checks and builds succeed. Product, Tauri, and workspace
crate versions must match. The workflow pins Rust 1.99 while Tauri's sidecar
copier depends on the Cargo build-directory layout used before Rust 1.100.

For a downloaded desktop package:

```sh
sudo apt install ./gp-client_0.2.0_amd64.deb
```

For the standalone archive, install the runtime dependencies described above,
extract it, and install the executables separately. This example uses
`gp-client-cli` so the desktop executable keeps its existing name:

```sh
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf gp-client-cli_0.2.0_linux-x86_64.tar.gz
cd gp-client-cli_0.2.0_linux-x86_64
sudo install -Dm755 gp-client /usr/local/bin/gp-client-cli
sudo install -Dm755 gp-helper /usr/local/libexec/gp-client/gp-helper
gp-client-cli --help
```
