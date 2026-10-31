# Cracked welcome fork

This branch customizes the fresh-session welcome artwork and branding using the
Rem pixel animation (`rem.piskel`, `rem.gif`) and `logo.txt`. The Windows package
is named **cracked**, with a deep-blue-to-lavender semantic color palette. The
packaging scripts and theme do not modify UI source. Normal Jcode functionality
remains available. This is a local fork, not a stock Jcode update channel.

## Rebuild welcome assets

From the repository root, with Python and Pillow available:

```powershell
python assets/custom/tools/build_welcome_assets.py assets/custom/rem.piskel
```

Commit the generated assets together with their source. `rem.piskel` is the
editable animation source. The generator is maintained with the custom assets.

## Windows prerequisites

Install Rust/Cargo using the MSVC toolchain and Visual Studio Build Tools with
"Desktop development with C++" plus a Windows SDK. Use Windows PowerShell 5.1
or newer. Git and Python are needed for source sync and asset generation,
respectively. Python is not needed to run the installed fork.

## Build and install

From the repository root:

```powershell
# Read-only dry run: resolves Cargo's target dir but does not build or install.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cracked/build.ps1 -WhatIf

# Builds and installs. Waits for other Cargo builds rather than killing them.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cracked/build.ps1
```

The build command is `cargo build --profile selfdev -p jcode --bin jcode`, with
`--message-format=json-render-diagnostics` so the installer can read the actual
executable artifact path. `[profile.selfdev]` inherits `release` but uses
`opt-level = 0`. Its default output is **`target/selfdev/jcode.exe`**, not
`target/release/jcode.exe`. Cargo metadata and the compiler artifact record
handle custom target directories and target triples without guessing the path.

The installer copies that executable and the launch helpers into:

```text
%LOCALAPPDATA%\jcode-cracked\bin\cracked.exe
%LOCALAPPDATA%\jcode-cracked\bin\cracked.cmd
%LOCALAPPDATA%\jcode-cracked\bin\seed.ps1
%LOCALAPPDATA%\jcode-cracked\bin\theme.toml
```

No PATH changes, stock executable replacement, process termination, server reload,
or stock config edits occur during installation. If Windows refuses to replace a
running `cracked.exe`, close only the fork's sessions/server and retry. Do not
kill the stock Jcode server. There is no need to copy the repository elsewhere.

### Why plain Cargo now works on Windows

The old global `[build].rustc-wrapper` referred to the POSIX shell script
`scripts/rustc-parallel-frontend`, which Windows cannot execute directly
(OS error 193). Cargo has no supported target-scoped `rustc-wrapper` key, so the
fork removes that global setting rather than putting it under an ignored
`target.'cfg(unix)'` table. No manual `--config` flag is needed:

```powershell
cargo check -p jcode-terminal-image
cargo build --profile selfdev -p jcode --bin jcode
```

Linux/macOS builds still work. The existing `scripts/dev_cargo.sh` enables the
parallel frontend on its supported Unix path. Direct Unix Cargo invocations can
opt in with `RUSTC_WRAPPER="$PWD/scripts/rustc-parallel-frontend" cargo check`.
Direct Cargo without that opt-in is portable but loses that optional speedup.
User-selected wrappers such as sccache are not overridden.

## Run side by side

**Always run the `.cmd` launcher, not `cracked.exe` directly.** A raw executable
inherits/defaults to stock Jcode paths and does not establish isolation.

```powershell
# Installed launcher, works from any working directory:
& "$env:LOCALAPPDATA\jcode-cracked\bin\cracked.cmd"

# Or, from the checkout after installation:
.\scripts\cracked\cracked.cmd

# Verify routing without launching, creating state, or touching config:
.\scripts\cracked\cracked.cmd --dry-run
```

From cmd.exe, use `"%LOCALAPPDATA%\jcode-cracked\bin\cracked.cmd"`.
Optionally add **only** `%LOCALAPPDATA%\jcode-cracked\bin` to your user PATH to
run `cracked` by name. The installer deliberately leaves PATH alone. The launcher
preserves your current working directory and forwards ordinary CLI arguments.
It adds `--no-update` to prevent automatic replacement by stock release binaries.
Rebuild this branch to update the fork. Avoid `/update`, `update`, and install or
self-development commands if you want to retain the custom build.

### Isolation and shared resources

| Resource | Fork location / behavior |
| --- | --- |
| Executable and launcher helpers | `%LOCALAPPDATA%\jcode-cracked\bin` |
| `JCODE_HOME` | `%LOCALAPPDATA%\jcode-cracked\home` |
| Configuration | `JCODE_HOME\config.toml` |
| `JCODE_RUNTIME_DIR` | `%LOCALAPPDATA%\jcode-cracked\run` |
| `JCODE_SOCKET` | `JCODE_RUNTIME_DIR\cracked.sock` |
| `JCODE_INSTALL_DIR` | `%LOCALAPPDATA%\jcode-cracked\bin` |
| Server name | `cracked` |
| Sessions, auth, logs, memory, app-owned config | Separate beneath the fork home/runtime directories |
| Project files and explicitly supplied environment variables | Same Windows user and working directory, not sandboxed |

The launcher replaces inherited stock routing variables and clears inherited
session/server identifiers and stale named-provider-profile selectors. Stock
profile names refer to a config the fork deliberately does not read, and inheriting
them can otherwise fail startup with "Unknown provider profile". Configure any
fork profile in its own config and select it with `--provider-profile`. The
launcher passes the isolated socket explicitly. This is
important: **`JCODE_HOME` alone does not isolate the server**. Socket defaults
come from the runtime directory, independently of home. On Windows the transport
hashes the full socket path into a named-pipe identifier, so the fork gets a
different pipe as well as different PID/lock/state locations. Nothing kills or
reloads the stock server as part of launch.

Implementation references: `crates/jcode-storage/src/lib.rs` (`jcode_dir`,
`runtime_dir`, `app_config_dir`, `user_home_path`),
`crates/jcode-base/src/config/config_file.rs` (`Config::path`),
`crates/jcode-app-core/src/server/socket.rs`, and
`crates/jcode-transport/src/windows.rs` (`path_to_pipe_name`).

### Configuration and credentials

First launch creates the fork home and seeds **only the checked-in auth-free
display theme**, plus `[display] theme = "dark"`. The scripts never read, print,
parse, or copy `%USERPROFILE%\.jcode\config.toml`, auth files, or API keys. They
do not import display settings from that secret-bearing file. An existing fork
config is left unchanged, including your own edits and login information.
Reinstalling refreshes the bundled theme but does not overwrite your fork config.
To change an existing palette, edit only the fork config's `[display.colors]`
table using `assets/custom/theme.toml` as a reference. Do not append duplicate
TOML tables. There is no global background role in `palette.rs`, so configure
your terminal background to **`#090b14`** separately. Panel backgrounds use the
near-black `user_bg` and dark-blue `selection_bg` roles.

**No stock credentials are shared automatically.** Sign in independently in the
fork, or explicitly provide supported provider environment variables in the
launching shell. Such environment variables are inherited, never copied to a
file or printed by these scripts. This is runtime-state isolation, not a security
sandbox against an agent intentionally accessing the same user's files.

Jcode supports consent-bound external credential reuse through
`[auth].trusted_external_source_paths` (and source-level
`trusted_external_sources` for sources without a stable file path). File-based
trust is bound to a canonical path, not just a broad source ID. However,
`JCODE_HOME` also redirects `user_home_path`, so this isolated launcher looks
for Claude/Codex credentials under `JCODE_HOME\external\.claude` and
`JCODE_HOME\external\.codex`, not your real home. Simply copying stock trust
settings would neither safely nor reliably enable reuse. The launcher therefore
does not import trust entries, create credential links, set legacy-auth bypasses,
or copy provider tokens. Prefer the fork's normal login flow. Any future explicit
external credential integration should use Jcode's own consent flow, not a
blanket config/auth copy.

Stock remains at `%LOCALAPPDATA%\jcode\bin\jcode.exe`, using
`%USERPROFILE%\.jcode\config.toml` and its own runtime/server. Leave those files
unchanged. To remove the fork, first close only its processes, then remove only
the `jcode-cracked` directory if you no longer need its sessions or credentials.

## Validate packaging

```powershell
cargo check -p jcode-terminal-image
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cracked/test.ps1
```

The packaging test checks PowerShell syntax, dry-run behavior, palette keys against
`palette.rs`, first-run seeding, and preservation of existing fork config. It uses
a temporary fake LocalAppData directory and never launches a server or reads the
stock config.

For an observable runtime check after installation (requires Python 3.11+):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/cracked/smoke.ps1
```

This copies only the installed executable and auth-free helpers into a temporary
package, launches its real `.cmd` wrapper with deliberately incorrect inherited
routing/profile selectors, and requires a matching IPC ping/pong from the fork's
unique Windows named pipe. It parses the actual first-run config as TOML and
checks that existing stock Jcode processes retain their PIDs and start times.
No model request is sent. Cleanup stops only the exact test child by its known
process handle, not processes by name, and removes only the temporary package.
On this Windows machine the runtime check observed an isolated server answering
ping, all 22 seeded colors, and three unchanged stock Jcode process identities.

## Sync upstream

Keep your custom work committed on `cracked-welcome` before merging. If needed,
configure `upstream` to the public Jcode repository first. Then, in cmd.exe or
PowerShell 7:

```text
git fetch upstream && git merge upstream/master
```

In Windows PowerShell 5.1, which lacks `&&`, run `git fetch upstream`, check that
it succeeded, and then run `git merge upstream/master`. Resolve conflicts without
discarding the custom assets, portable Cargo config, or isolated launch scripts.
Rebuild the assets if necessary, rerun the packaging tests, and rebuild/install.
These commands fetch and merge locally. They do not push.

## Make it your main jcode

Opt-in alternative to isolation above. From this checkout, close **all** main
Jcode clients/servers, unset `JCODE_HOME` and any true `JCODE_CHECK_UPDATES`
override, then run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\test-apply.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\apply-to-main.ps1 -WhatIf
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\apply-to-main.ps1
# One-command rollback, after closing main Jcode again:
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\rollback-main.ps1
```

Apply resolves Cargo's `selfdev` compiler artifact like `build.ps1`, backs up
`%LOCALAPPDATA%\jcode\bin\jcode.exe` and `%USERPROFILE%\.jcode\config.toml`,
then swaps/verifies the executable and surgically sets
`[features] check_updates = false`. Add `-Theme` for `[display.colors]` only, or
`-ArtifactPath 'C:\path\jcode.exe'` for a prebuilt executable. Formatting/BOM and
unrelated settings stay intact. Ambiguous TOML fails closed. `-WhatIf` prints
steps and a redacted key-only diff without building, running binaries or writing
installation/config files. Existing sessions, logins and trusted external paths
stay in place with **no re-auth**. Auth files/keys are never opened/copied/changed.

The update gate is `src/cli/startup.rs:488-507`; its environment override is
`crates/jcode-base/src/config/env_overrides.rs:376-379`. Leave `JCODE_HOME` unset:
`crates/jcode-storage/src/lib.rs::user_home_path` redirects external Claude/Codex
credentials whenever it exists. Avoid explicit update/install/selfdev/reload
commands. `update_channel = "manual"` falls back to stable, not disabled.
Current/stable/shared-server channels are untouched, so servers may still use
their previous channel. Use ordinary `jcode`, not isolated `cracked.cmd`.

Backups are `jcode.exe.stock-<UTC timestamp>` and `config.toml.bak-<same timestamp>`.
Rollback restores the newest paired transaction byte-for-byte, including its
previous update preference, retaining backups. Save post-apply config changes
first and keep config backups private. Detected install failures recover from
backups. Both scripts refuse running main clients/servers, including channel
builds. `-Force` only waits `-WaitSeconds 60` for manual closure, never kills or
bypasses checks. Do not launch Jcode or edit config during a swap.

The sandbox test uses only TEMP dummy binaries and auth-free fixtures. Repeat
the real-path **dry-run only**, verifying SHA256, sizes, creation/write/access
timestamps and backup inventory, with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\cracked\test-apply.ps1 -RealDryRunOnly
```
