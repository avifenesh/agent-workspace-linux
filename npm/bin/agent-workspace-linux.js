#!/usr/bin/env node
// agent-workspace-linux.js — thin launcher for the native MCP server binary.
//
// Why a JS launcher instead of pointing `bin` directly at the native binary?
// - npm resolves `bin` entries from the tarball at install time; the downloaded
//   binary is placed there by postinstall, which runs AFTER npm creates shims.
//   Pointing `bin` at the native binary directly is therefore unreliable across
//   npm versions and install modes.
// - The JS shim is always present in the tarball, so npm can always create the
//   global shim. At runtime the shim locates the native binary relative to its
//   own __dirname and execs it.
//
// This pattern mirrors what esbuild, @swc/core, and rollup use for their own
// native binaries published via npm.
//
// Signal handling: SIGINT is ignored in this parent process so the child MCP
// server handles Ctrl-C itself without the shell printing a double ^C.

"use strict";

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const binaryPath = path.join(__dirname, "agent-workspace-linux");

if (!fs.existsSync(binaryPath)) {
  console.error(
    "agent-workspace-linux: native binary not found at " + binaryPath + "\n" +
    "The binary is downloaded during installation (postinstall). " +
    "If --ignore-scripts was used, run:\n" +
    "  node " + path.join(__dirname, "..", "scripts", "postinstall.js")
  );
  process.exit(1);
}

// Ignore SIGINT in the launcher: let the child (MCP server) handle its own
// shutdown. Without this both processes would receive the signal and the child
// might not get a chance to flush stdio cleanly.
process.on("SIGINT", () => {});

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  // Detach false (default): child shares the session, gets signals from tty.
});

child.on("close", (code, signal) => {
  if (signal) {
    // Propagate signal so the parent's exit looks like a signalled death to
    // any caller (e.g. npm run scripts that check $?).
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 0);
  }
});

child.on("error", (err) => {
  console.error("agent-workspace-linux: failed to start binary —", err.message);
  process.exit(1);
});
