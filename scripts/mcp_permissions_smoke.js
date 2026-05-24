#!/usr/bin/env node
"use strict";

const childProcess = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..");
const bin =
  process.env.AGENT_WORKSPACE_BIN ||
  path.join(repoRoot, "target", "debug", "agent-workspace-linux");

if (!fs.existsSync(bin)) {
  throw new Error(`agent-workspace-linux binary not found at ${bin}; run cargo build first`);
}

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "agent-workspace-mcp-smoke-"));
const permissionsPath = path.join(tempDir, "permissions.json");
const configDir = path.join(tempDir, "config");
const runtimeDir = path.join(tempDir, "runtime");
fs.mkdirSync(configDir, { recursive: true });
fs.mkdirSync(runtimeDir, { recursive: true });
fs.writeFileSync(
  permissionsPath,
  `${JSON.stringify(
    {
      network: { mode: "disabled" },
      apps: { allow: ["sh"] },
    },
    null,
    2,
  )}\n`,
);

const child = childProcess.spawn(bin, ["mcp", "--permissions", permissionsPath], {
  cwd: repoRoot,
  env: {
    ...process.env,
    XDG_CONFIG_HOME: configDir,
    XDG_RUNTIME_DIR: runtimeDir,
  },
  stdio: ["pipe", "pipe", "pipe"],
});

let nextId = 1;
let stdoutBuffer = "";
let stderr = "";
const pending = new Map();

child.stderr.on("data", (chunk) => {
  stderr += String(chunk);
});

child.stdout.on("data", (chunk) => {
  stdoutBuffer += String(chunk);
  for (;;) {
    const newlineIndex = stdoutBuffer.indexOf("\n");
    if (newlineIndex === -1) return;
    const line = stdoutBuffer.slice(0, newlineIndex).trim();
    stdoutBuffer = stdoutBuffer.slice(newlineIndex + 1);
    if (!line) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      fail(`invalid JSON-RPC line from MCP server: ${line}`);
    }
    const slot = pending.get(message.id);
    if (!slot) continue;
    pending.delete(message.id);
    clearTimeout(slot.timer);
    if (message.error) {
      slot.reject(new Error(JSON.stringify(message.error)));
    } else {
      slot.resolve(message.result);
    }
  }
});

child.on("exit", (code) => {
  for (const slot of pending.values()) {
    clearTimeout(slot.timer);
    slot.reject(new Error(`MCP server exited before response, code=${code}, stderr=${stderr}`));
  }
  pending.clear();
});

function fail(message) {
  try {
    child.kill("SIGTERM");
  } catch {
    // ignore cleanup races
  }
  throw new Error(message);
}

function request(method, params, timeoutMs = 5000) {
  const id = nextId++;
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`timed out waiting for ${method}`));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
  });
}

function notify(method, params) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
}

async function callTool(name, args) {
  const result = await request("tools/call", {
    name,
    arguments: args || {},
  });
  if (result.isError) {
    throw new Error(`tool ${name} returned MCP error: ${JSON.stringify(result)}`);
  }
  if (result.structuredContent && typeof result.structuredContent === "object") {
    return result.structuredContent;
  }
  const text = result.content?.find((entry) => entry?.type === "text")?.text;
  return text ? JSON.parse(text) : null;
}

function assert(condition, message) {
  if (!condition) fail(message);
}

async function main() {
  await request("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "agent-workspace-linux-smoke", version: "0" },
  });
  notify("notifications/initialized", {});

  const permissions = await callTool("mcp_permissions");
  assert(permissions.configured === true, "mcp_permissions did not report configured=true");
  assert(permissions.restricted === true, "mcp_permissions did not report restricted=true");
  assert(
    permissions.ceiling?.network?.mode === "disabled",
    "mcp_permissions did not report disabled network ceiling",
  );

  const tooOpen = await callTool("profile_put", {
    dry_run: true,
    profile: {
      id: "too-open",
      network: { mode: "inherit_host" },
      mounts: [],
      setup_commands: [],
      startup_apps: [],
    },
  });
  assert(tooOpen.ok === false, "profile_put should reject inherited network under disabled ceiling");
  assert(
    String(tooOpen.message || "").includes("exceeds MCP permission ceiling"),
    "profile_put rejection did not mention the permission ceiling",
  );

  const disallowedApp = await callTool("profile_put", {
    dry_run: true,
    profile: {
      id: "bad-app",
      network: { mode: "disabled" },
      mounts: [],
      setup_commands: [],
      startup_apps: [{ command: ["bash", "-lc", "true"] }],
    },
  });
  assert(disallowedApp.ok === false, "profile_put should reject app outside allowlist");
  assert(
    String(disallowedApp.message || "").includes("not allowed by the MCP app allowlist"),
    "profile_put app rejection did not mention the app allowlist",
  );

  const allowed = await callTool("profile_put", {
    dry_run: true,
    profile: {
      id: "allowed",
      network: { mode: "disabled" },
      mounts: [],
      setup_commands: [],
      startup_apps: [{ command: ["sh", "-lc", "true"] }],
    },
  });
  assert(allowed.ok === true, "profile_put dry-run should allow narrowed profile");
  assert(allowed.saved === false && allowed.would_create === true, "profile_put dry-run shape changed");

  child.stdin.end();
  await new Promise((resolve, reject) => {
    child.once("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`MCP server exited with code ${code}, stderr=${stderr}`));
    });
  });
}

main()
  .then(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
    console.log("mcp permissions smoke passed");
  })
  .catch((error) => {
    try {
      child.kill("SIGTERM");
    } catch {
      // ignore cleanup races
    }
    console.error(error instanceof Error ? error.stack || error.message : String(error));
    console.error(`preserved temp dir: ${tempDir}`);
    process.exit(1);
  });
