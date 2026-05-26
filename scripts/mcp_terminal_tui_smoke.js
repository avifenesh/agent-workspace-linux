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

function commandExists(name) {
  const resolved = childProcess.spawnSync("sh", ["-lc", `command -v ${name}`], {
    encoding: "utf8",
  });
  return resolved.status === 0 && resolved.stdout.trim();
}

if (!commandExists("xterm") || !commandExists("tmux")) {
  console.log("workspace terminal TUI smoke skipped: xterm or tmux not found");
  process.exit(0);
}

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "awl-tui-"));
const configDir = path.join(tempDir, "config");
const runtimeDir = path.join(tempDir, "runtime");
fs.mkdirSync(configDir, { recursive: true });
fs.mkdirSync(runtimeDir, { recursive: true });
fs.chmodSync(runtimeDir, 0o700);

const env = {
  ...process.env,
  XDG_CONFIG_HOME: configDir,
  XDG_RUNTIME_DIR: runtimeDir,
};

const child = childProcess.spawn(bin, ["mcp", "--headless"], {
  cwd: repoRoot,
  env,
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
    } catch {
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

function abort(message) {
  try {
    child.kill("SIGTERM");
  } catch {
    // ignore cleanup races
  }
  throw new Error(message);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
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
  const result = await request(
    "tools/call",
    {
      name,
      arguments: args || {},
    },
    timeoutMs,
    `tools/call ${name}`,
  );
  if (result.isError) {
    throw new Error(`tool ${name} returned MCP error: ${JSON.stringify(result)}`);
  }
  if (result.structuredContent && typeof result.structuredContent === "object") {
    return result.structuredContent;
  }
  const text = result.content?.find((entry) => entry?.type === "text")?.text;
  return text ? JSON.parse(text) : null;
}

async function waitForTerminalText(workspaceId, terminalId, pattern) {
  const deadline = Date.now() + 10000;
  let last;
  while (Date.now() < deadline) {
    last = await callTool("workspace_terminal_read", {
      id: workspaceId,
      terminal_id: terminalId,
      preserve_trailing_spaces: true,
    });
    if (pattern.test(last.screen?.text || "")) return last;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`terminal text did not match ${pattern}: ${JSON.stringify(last)}`);
}

async function main() {
  const workspaceId = `tui-${process.pid}`;
  const terminalId = "game";
  try {
    const initializeResult = await request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "agent-workspace-linux-terminal-tui-smoke", version: "0" },
    });
    notify("notifications/initialized", {});
    assert(
      /workspace_terminal_read/.test(String(initializeResult.instructions || "")),
      `initialize instructions should expose terminal text controls: ${initializeResult.instructions}`,
    );

    const started = await callTool(
      "workspace_start",
      {
        id: workspaceId,
        acknowledge_hidden_workspace: true,
        purpose: "Direct MCP terminal TUI smoke",
        width: 900,
        height: 620,
      },
      15000,
    );
    assert(started.ok === true, `workspace_start failed: ${JSON.stringify(started)}`);

    const launched = await callTool(
      "workspace_run_in_terminal",
      {
        id: workspaceId,
        terminal_id: terminalId,
        title: "terminal-tui-smoke",
        command: [
          "sh",
          "-lc",
          "printf 'terminal-ready\\n'; read line; printf 'got:%s\\n' \"$line\"; while :; do sleep 1; done",
        ],
        window_timeout_ms: 15000,
        timeout_ms: 10000,
      },
      20000,
    );
    assert(
      launched.ok === true &&
        launched.terminal?.terminal_id === terminalId &&
        launched.target_handles?.terminal_ids?.includes(terminalId),
      `workspace_run_in_terminal should return terminal handles: ${JSON.stringify(launched)}`,
    );

    const ready = await waitForTerminalText(workspaceId, terminalId, /terminal-ready/);
    assert(
      /terminal-ready/.test(ready.screen?.text || ""),
      `workspace_terminal_read should capture terminal text: ${JSON.stringify(ready)}`,
    );

    const input = await callTool(
      "workspace_terminal_input",
      {
        id: workspaceId,
        terminal_id: terminalId,
        text: "hello-from-batch",
        keys: ["Return"],
      },
      10000,
    );
    assert(
      input.ok === true &&
        input.input?.normalized_keys?.includes("Enter") &&
        input.input?.text_bytes === "hello-from-batch".length,
      `workspace_terminal_input should send literal text and batched keys: ${JSON.stringify(input)}`,
    );

    const echoed = await waitForTerminalText(workspaceId, terminalId, /got:hello-from-batch/);
    assert(
      /got:hello-from-batch/.test(echoed.screen?.text || ""),
      `workspace_terminal_read should see post-input TUI state: ${JSON.stringify(echoed)}`,
    );

    const events = await callTool("workspace_events", { id: workspaceId, tail: 20 });
    const terminalEvent = (events.events || []).find((event) => event.kind === "terminal_input");
    assert(terminalEvent, `terminal input should record metadata event: ${JSON.stringify(events.events)}`);
    assert(
      terminalEvent.detail?.text_bytes === "hello-from-batch".length &&
        !Object.hasOwn(terminalEvent.detail || {}, "text"),
      `terminal input event should omit raw text: ${JSON.stringify(terminalEvent)}`,
    );

    const stopped = await callTool("workspace_stop", { id: workspaceId }, 15000);
    assert(stopped.ok === true, `workspace_stop failed: ${JSON.stringify(stopped)}`);
  } finally {
    try {
      await callTool("workspace_stop", { id: workspaceId }, 5000);
    } catch {
      // ignore cleanup races
    }
    try {
      child.kill("SIGTERM");
    } catch {
      // ignore cleanup races
    }
  }
}

main()
  .then(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
    console.log("workspace terminal TUI smoke passed");
  })
  .catch((error) => {
    console.error(error && error.stack ? error.stack : error);
    console.error(stderr);
    console.error(`preserved temp dir: ${tempDir}`);
    try {
      child.kill("SIGTERM");
    } catch {
      // ignore cleanup races
    }
    process.exit(1);
  });
