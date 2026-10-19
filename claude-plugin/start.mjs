import { spawn, execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, delimiter, dirname } from "node:path";
import { createInterface } from "node:readline";

const PLUGIN_DIR = "com_github_nextbuilder_voltaris";
const win = process.platform === "win32";
const exe = win ? "voltaris.exe" : "voltaris";
const home = homedir();

function kicadConfigDir() {
  if (win) return join(process.env.APPDATA || join(home, "AppData", "Roaming"), "kicad", "10.0");
  if (process.platform === "darwin") return join(home, "Library", "Preferences", "kicad", "10.0");
  return join(process.env.XDG_CONFIG_HOME || join(home, ".config"), "kicad", "10.0");
}

function kicadCommon() {
  try {
    return JSON.parse(readFileSync(join(kicadConfigDir(), "kicad_common.json"), "utf8"));
  } catch {
    return {};
  }
}

function windowsDocuments() {
  try {
    const out = execFileSync("reg", ["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders", "/v", "Personal"], { encoding: "utf8", windowsHide: true });
    const m = out.match(/Personal\s+REG_\w+\s+(.+)/);
    return m ? m[1].trim().replace(/%([^%]+)%/g, (_, v) => process.env[v] ?? `%${v}%`) : null;
  } catch {
    return null;
  }
}

function documentsDirs() {
  const docs = [join(home, "Documents")];
  if (win) {
    const known = windowsDocuments();
    if (known) docs.unshift(known);
    for (const v of ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]) if (process.env[v]) docs.push(join(process.env[v], "Documents"));
    try {
      for (const d of readdirSync(home)) if (d.startsWith("OneDrive")) docs.push(join(home, d, "Documents"));
    } catch {}
  }
  return [...new Set(docs)];
}

function candidates() {
  const list = [];
  const vars = kicadCommon().environment?.vars || {};
  if (process.env.VOLTARIS_BIN) list.push(process.env.VOLTARIS_BIN);
  for (const third of [process.env.KICAD10_3RD_PARTY, vars.KICAD10_3RD_PARTY]) if (third) list.push(join(third, "plugins", PLUGIN_DIR, "bin", exe));
  const docs = documentsDirs().map((d) => join(d, "KiCad"));
  if (vars.KICAD_DOCUMENTS_HOME) docs.unshift(vars.KICAD_DOCUMENTS_HOME);
  for (const d of docs) list.push(join(d, "10.0", "3rdparty", "plugins", PLUGIN_DIR, "bin", exe));
  list.push(join(home, ".local", "share", "kicad", "10.0", "3rdparty", "plugins", PLUGIN_DIR, "bin", exe));
  for (const dir of (process.env.PATH || "").split(delimiter)) if (dir) list.push(join(dir, exe));
  return list;
}

const binary = candidates().find((p) => existsSync(p));

if (binary) {
  const env = { ...process.env };
  if (!env.KICAD_CLI) {
    const py = kicadCommon().api?.interpreter_path;
    const cli = py && join(dirname(py), win ? "kicad-cli.exe" : "kicad-cli");
    if (cli && existsSync(cli)) env.KICAD_CLI = cli;
  }
  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit", env });
  for (const sig of ["SIGINT", "SIGTERM"]) process.on(sig, () => child.kill(sig));
  child.on("exit", (code, signal) => (signal ? process.kill(process.pid, signal) : process.exit(code ?? 0)));
} else {
  const help =
    "Voltaris is not installed yet. In KiCad 10 open Plugin and Content Manager, choose Install from File, " +
    "and pick voltaris-pcm-v<version>-<platform>.zip from https://github.com/NextBuilder/Voltaris/releases. " +
    "Restart KiCad, enable Preferences > Plugins > KiCad API, then restart Claude. " +
    "If the server lives elsewhere, set VOLTARIS_BIN to the full path of " + exe + ". Looked in: " + candidates().join("; ");
  const send = (msg) => process.stdout.write(JSON.stringify(msg) + "\n");
  createInterface({ input: process.stdin }).on("line", (line) => {
    let req;
    try { req = JSON.parse(line); } catch { return; }
    if (req.id === undefined) return;
    const reply = (result) => send({ jsonrpc: "2.0", id: req.id, result });
    if (req.method === "initialize") {
      reply({ protocolVersion: req.params?.protocolVersion ?? "2025-06-18", capabilities: { tools: {} }, serverInfo: { name: "voltaris-setup", version: "1" } });
    } else if (req.method === "tools/list") {
      reply({ tools: [{ name: "voltaris_setup", title: "Voltaris Setup", description: "Explains how to install the Voltaris server, which is not installed on this computer yet.", inputSchema: { type: "object", properties: {} }, annotations: { readOnlyHint: true, openWorldHint: false } }] });
    } else if (req.method === "tools/call") {
      reply({ content: [{ type: "text", text: help }], isError: true });
    } else if (req.method === "ping") {
      reply({});
    } else {
      send({ jsonrpc: "2.0", id: req.id, error: { code: -32601, message: "Method not found" } });
    }
  });
}
