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
const stopViaMcp = process.env.AGENT_WORKSPACE_MCP_STOP !== "0";

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

const childEnv = {
  ...process.env,
  XDG_CONFIG_HOME: configDir,
  XDG_RUNTIME_DIR: runtimeDir,
};

const child = childProcess.spawn(bin, ["mcp", "--permissions", permissionsPath], {
  cwd: repoRoot,
  env: childEnv,
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
      abort(`invalid JSON-RPC line from MCP server: ${line}`);
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

child.on("exit", (code, signal) => {
  for (const slot of pending.values()) {
    clearTimeout(slot.timer);
    slot.reject(
      new Error(
        `MCP server exited before ${slot.method} response, code=${code}, signal=${signal}, stderr=${stderr}`,
      ),
    );
  }
  pending.clear();
});

function fail(message) {
  throw new Error(message);
}

function abort(message) {
  try {
    child.kill("SIGTERM");
  } catch {
    // ignore cleanup races
  }
  throw new Error(message);
}

function request(method, params, timeoutMs = 5000, label = method) {
  const id = nextId++;
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`timed out waiting for ${label}`));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer, method: label });
  });
}

function notify(method, params) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
}

async function callTool(name, args, timeoutMs) {
  const result = await request("tools/call", {
    name,
    arguments: args || {},
  }, timeoutMs, `tools/call ${name}`);
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
    dry_run: false,
    profile: {
      id: "allowed",
      network: { mode: "disabled" },
      mounts: [],
      setup_commands: [],
      startup_apps: [{ command: ["sh", "-lc", "true"] }],
    },
  });
  assert(allowed.ok === true, "profile_put should allow narrowed profile");
  assert(allowed.saved === true && allowed.created === true, "profile_put save shape changed");

  const workspaceId = `mcp-smoke-${process.pid}`;
  const started = await callTool(
    "workspace_start",
    {
      id: workspaceId,
      profile: "allowed",
      acknowledge_hidden_workspace: true,
      purpose: "MCP compact response smoke",
    },
    15000,
  );
  assert(started.ok === true, "workspace_start should create the smoke workspace");

  try {
    const run = await callTool(
      "workspace_run_app",
      {
        id: workspaceId,
        name: "mcp-compact-probe",
        profile: "allowed",
        command: ["sh", "-lc", "echo mcp-run-ok"],
        timeout_ms: 5000,
        tail_bytes: 2000,
      },
      15000,
    );
    assert(
      run.ok === true && run.run?.succeeded === true,
      `workspace_run_app did not succeed: ${JSON.stringify(run)}`,
    );
    assert(
      run.run?.stdout?.content?.includes("mcp-run-ok"),
      "workspace_run_app stdout did not include expected output",
    );
    assert(
      Array.isArray(run.run?.launch?.apps) && run.run.launch.apps.length === 1,
      "workspace_run_app launch did not include the affected app",
    );
    assert(
      Array.isArray(run.run?.wait?.apps) && run.run.wait.apps.length === 1,
      "workspace_run_app wait did not include the affected app",
    );
    assert(
      Array.isArray(run.run?.launch?.status?.apps) && run.run.launch.status.apps.length === 0,
      "workspace_run_app launch status should not include full app history",
    );
    assert(
      Array.isArray(run.run?.wait?.status?.apps) && run.run.wait.status.apps.length === 0,
      "workspace_run_app wait status should not include full app history",
    );

    const status = await callTool("workspace_status", { id: workspaceId }, 5000);
    assert(
      Array.isArray(status.apps) && status.apps.length >= 1,
      "workspace_status should still expose full app history",
    );
    assert(
      Array.isArray(status.status?.apps) && status.status.apps.length >= 1,
      "workspace_status status should still expose full app history",
    );
  } finally {
    if (stopViaMcp) {
      const stopped = await callTool("workspace_stop", { id: workspaceId }, 15000);
      assert(
        stopped.ok === true && stopped.status?.ready === false,
        `workspace_stop did not stop the smoke workspace: ${JSON.stringify(stopped)}`,
      );
    } else {
      childProcess.spawnSync(bin, ["workspace", "stop", "--id", workspaceId], {
        cwd: repoRoot,
        env: childEnv,
        stdio: "ignore",
        timeout: 15000,
      });
    }
  }

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
