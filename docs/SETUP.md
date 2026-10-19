# Setup & configuration

Everything beyond the quick install in the [README](../README.md): manual installs, building from source, client configuration and platform notes.

## How it works

| Layer | Mechanism |
|-------|-----------|
| Schematic editing | Direct `.kicad_sch` S-expression editing with atomic writes (no KiCad required) |
| PCB editing | KiCad 10 IPC API (NNG + protobuf) — real-time and undo-aware; single-footprint placement has a safe headless fallback |
| Specctra routing | Revision-bound Rust DSN export, local Freerouting MCP routing, and strict one-transaction SES import; an authenticated KiCad 10 native-export bridge is explicit opt-in |
| Exports & checks | `kicad-cli` subprocess (Gerber, PDF, ERC, DRC, …) |
| Transport | MCP JSON-RPC over stdio (default), or Streamable HTTP (`transport = "http"` / `"both"`) |

## Installation

### From the KiCad Plugin Manager (recommended)

1. Download the package for your OS from [Releases](https://github.com/NextBuilder/Voltaris/releases):
   `voltaris-pcm-v<version>-windows.zip`, `-macos.zip`, or `-linux.zip`. Each
   bundles that platform's server binary — the macOS package is a universal
   build, so one download covers Apple Silicon and Intel. (The `voltaris-pcm-*`
   assets are the KiCad plugin packages; the other archives are standalone
   server binaries.)
2. Open KiCad 10 → **Plugin and Content Manager**
3. Click **Install from File** and select the zip
4. Restart KiCad

Verify: open the **PCB Editor** → **Tools → External Plugins** → you should see
**Voltaris**.

For KiCad 10, the Voltaris settings dialog also offers an optional **native
Specctra bridge**. When enabled, `export_specctra_dsn` can ask the active PCB
Editor to generate its native DSN while Voltaris still binds the export to the
exact IPC snapshot and creates the strict reverse manifest used during SES
import. The bridge is local-only, authenticated, disabled by default, and not
the KiCad 11 integration path. Voltaris uses its Rust exporter by default;
`native_bridge_mode: "prefer"` enables fallback to Rust when the bridge is
unavailable, while `"require"` refuses instead.

For end-to-end autorouting, run `check_freerouting`, then
`export_specctra_dsn` → `route_specctra_dsn` →
`plan_specctra_ses_import` / `apply_specctra_ses`. Readiness reports engine
discovery, native-MCP compatibility, and complete bridge availability
separately. The owned Java child is loopback-only and is reaped on success,
failure, timeout, or cancellation. The first supported profile preserves fixed
straight tracks and through vias; unlocked routing, arcs, zones, and unsupported
geometry are rejected before mutation.

### Build from source

```bash
# protoc is required (protobuf code generation), and cmake (the nng crate
# compiles the NNG C library with it).
# Windows: choco install protoc cmake
# macOS:   brew install protobuf cmake
# Linux:   apt install protobuf-compiler cmake
cargo build --release -p voltaris
```

### Install guidance for your AI client

Voltaris bundles shared KiCad skills for Claude and Codex. Select the client when
installing, checking, or removing that guidance:

```bash
# Existing behavior remains the default: Claude skills, agents, and hooks
voltaris init

# Codex installs only the shared skills under ~/.agents/skills
voltaris init --client codex
voltaris status --client codex
voltaris uninstall --client codex
```

MCP server startup never installs or restores guidance. Run `voltaris init`
explicitly when you want those files installed; after `voltaris uninstall`,
starting the server leaves them removed.

Guidance written by an older `voltaris init` is not updated by upgrading the
binary. `voltaris status` compares every installed skill, agent, and hook with
the bundle in the binary you run and marks each `current`, `different`, or
`missing`. The server makes the same check once for each installed version:
the first start after `voltaris init` compares the files, records
the result in `~/.voltaris`, and adds a one-line notice to its `initialize`
instructions if guidance is out of sync. Later starts reuse that record, so
guidance you chose to keep is not reported again. `get_installation_info`
shows the result. Re-run `voltaris init` to update it.
It overwrites differing files, so keep a copy of any file you edited. `--client` remains accepted in server
commands for compatibility. For example, register a standalone binary with the
Codex CLI using:

```bash
codex mcp add voltaris -- /path/to/voltaris --client codex
```

Claude remains the default when `--client` is omitted. The installer tracks the
two clients independently, and a Codex install does not create or modify
`~/.claude`.

`--help` on any subcommand prints that subcommand's usage and writes nothing —
`voltaris init --help` describes the installer rather than running it. An
argument Voltaris does not recognise is an error rather than being ignored, so a
typo such as `--cleint codex` stops instead of quietly installing for the
default client.

To verify which Voltaris process an MCP client is actually using, call the
always-visible `get_installation_info` tool. It reports the serving build,
executable path, verified installation source when one can be proven, KiCad
CLI and IPC detection, and restart guidance. A missing build commit or an
`unknown` installation source means the available evidence was insufficient;
it is not silently guessed from a directory name.

### macOS

The [Releases](https://github.com/NextBuilder/Voltaris/releases) page ships
standalone server binaries for both Apple Silicon (`aarch64-apple-darwin`) and
Intel (`x86_64-apple-darwin`). They are not yet code-signed, so if you download
one through a browser, clear the quarantine flag before first launch:

```bash
tar xzf voltaris-v*-aarch64-apple-darwin.tar.gz
xattr -d com.apple.quarantine ./voltaris   # only needed for browser downloads
./voltaris --help
```

Or build from source as above (verified on Apple Silicon; the same
`target/release/voltaris` binary is the MCP server).

KiCad on macOS keeps its tools inside the app bundle and they are not on
`PATH`, so point Voltaris at them in `~/Library/Application Support/voltaris/config.toml`:

```toml
kicad_cli = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"
kicad_binary = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad"
# KiCad 10's IPC socket on macOS (enable it in KiCad:
# Preferences → Plugins → "Enable KiCad API")
ipc_address = "ipc:///tmp/kicad/api.sock"
```

Claude Desktop's config lives at
`~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "voltaris": {
      "command": "/path/to/voltaris"
    }
  }
}
```

For Claude Code, put the same snippet in a `.mcp.json` in your project root.

The macOS PCM package (`voltaris-pcm-v<version>-macos.zip`) bundles a universal
server binary, so one download covers Apple Silicon and Intel. The schematic
viewer compiles and launches on macOS (Tauri 2 uses the system WKWebView —
WebView2 is only a Windows requirement) but hasn't had the same mileage as
the Windows build yet.

## Setup with Claude Desktop

After a PCM install, the server binary lives in your KiCad documents folder:

```
C:\Users\<YOU>\Documents\KiCad\10.0\3rdparty\plugins\com_github_nextbuilder_voltaris\bin\voltaris.exe
```

Edit `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "voltaris": {
      "command": "C:\\Users\\<YOU>\\Documents\\KiCad\\10.0\\3rdparty\\plugins\\com_github_nextbuilder_voltaris\\bin\\voltaris.exe"
    }
  }
}
```

Restart Claude Desktop and the Voltaris tools appear. For Claude Code, drop the same
snippet into a `.mcp.json` in your project root (see [examples/](examples/)).

## Schematic viewer

A standalone viewer that auto-refreshes as the schematic file changes:

```bash
schematic-viewer.exe path\to\your\root_schematic.kicad_sch
```

Point it at the root sheet of a hierarchical design and every sub-sheet is rendered
too, with a depth-indented sheet selector in the toolbar. Edits saved from KiCad (or
made by the AI through the schematic tools) re-render only the sheets that changed
and refresh the view live — rendering runs against temp-folder snapshots, so the
viewer never blocks KiCad from saving. Pan with click-drag, zoom with the wheel,
`0` to fit, `R` to refresh, drag-and-drop to open a different file. Also launchable
by the AI via the `open_schematic_viewer` tool.

Needs the WebView2 runtime (pre-installed on Windows 10/11) and a KiCad install for
`kicad-cli` (auto-discovered, or pass `--kicad-cli <path>`). Built separately from
the main workspace — see [DEVELOPMENT.md](DEVELOPMENT.md) for build steps.

## Requirements

- KiCad 10 (Windows is the most-tested platform; macOS works from the release
  binaries or a source build — see the [macOS section](#macos) above. Linux
  compiles and passes tests in CI but hasn't had per-platform QA yet; both are
  tracked on the [roadmap](ROADMAP.md))
- `kicad-cli` (ships with KiCad — used for exports, ERC, DRC)
- For most PCB tools: KiCad running with the target board open (IPC API).
  `place_component`, `move_component`, `rotate_component`, and (on KiCad
  10.0.6+) `flip_component` can safely fall back to a closed board file when
  IPC is unreachable. An older reachable KiCad makes `flip_component` fail
  closed with `unsupported_capability`; it never edits the file underneath a
  live editor that cannot perform the native flip.
## Troubleshooting

**Plugin doesn't appear in KiCad** — install via the Plugin and Content Manager (not
manual copy), then restart KiCad.

**PCB tools return "KiCad must be running with the board loaded"** — open KiCad
with that board first; most PCB tools talk to the running PCB editor. Only an
unreachable KiCad produces that message: if KiCad is running and the tool
refuses anyway, the error is the tool's own reason for refusing.
`place_component`, `move_component`, `rotate_component`, and (on KiCad
10.0.6+) `flip_component` can fall back to a closed board file when no KiCad
process is reachable. An older, reachable KiCad makes `flip_component` refuse
rather than silently editing the file underneath it.

See [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for socket setup, tools
that don't appear after `load_toolset`, and transaction recovery.

**"kicad-cli not found"** — common install paths are auto-detected; set the path
explicitly in the plugin settings dialog, in a `settings.json` beside the binary,
or in a `voltaris.toml` in the working directory. (A file under any other name
works only when passed with `--config`.)
