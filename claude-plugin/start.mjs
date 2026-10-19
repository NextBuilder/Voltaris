import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join, delimiter } from "node:path";
import { createInterface } from "node:readline";

const PLUGIN_DIR = "com_github_nextbuilder_voltaris";
const exe = process.platform === "win32" ? "voltaris.exe" : "voltaris";

function candidates() {
  const home = homedir();
  const list = [];
  if (process.env.VOLTARIS_BIN) list.push(process.env.VOLTARIS_BIN);
  if (process.env.KICAD10_3RD_PARTY) list.push(join(process.env.KICAD10_3RD_PARTY, "plugins", PLUGIN_DIR, "bin", exe));
  const docs = [join(home, "Documents")];
  if (process.env.OneDrive) docs.push(join(process.env.OneDrive, "Documents"));
  for (const d of docs) list.push(join(d, "KiCad", "10.0", "3rdparty", "plugins", PLUGIN_DIR, "bin", exe));
  list.push(join(home, ".local", "share", "kicad", "10.0", "3rdparty", "plugins", PLUGIN_DIR, "bin", exe));
  for (const dir of (process.env.PATH || "").split(delimiter)) if (dir) list.push(join(dir, exe));
  return list;
}

const binary = candidates().find((p) => existsSync(p));

if (binary) {
  const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
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
