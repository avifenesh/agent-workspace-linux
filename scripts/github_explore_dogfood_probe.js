#!/usr/bin/env node
"use strict";

const childProcess = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..");
const desktopRepo = process.env.CODEX_DESKTOP_LINUX_REPO || path.join(repoRoot, "..", "codex-desktop-linux");
const args = process.argv.slice(2);
const selfTest = args.includes("--self-test");
const bin =
  process.env.AGENT_WORKSPACE_BIN ||
  process.env.BIN ||
  path.join(repoRoot, "target", "debug", "agent-workspace-linux");
const targetUrl = process.env.GITHUB_EXPLORE_URL || "https://github.com/explore";
const reportDir = process.env.REPORT_DIR || path.join(repoRoot, "target", "github-explore-dogfood");
const openViewer =
  process.env.GITHUB_EXPLORE_OPEN_VIEWER !== "0" && process.env.GITHUB_EXPLORE_NO_VIEWER !== "1";
const holdSeconds = Number(process.env.GITHUB_EXPLORE_HOLD_SECONDS || "0");
const pageWaitMs = Number(process.env.GITHUB_EXPLORE_PAGE_WAIT_MS || "3000");
let cachedSourceIdentity = null;

if (!selfTest && !fs.existsSync(bin)) {
  throw new Error(`agent-workspace-linux binary not found at ${bin}; run cargo build first`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function resolveBrowser() {
  if (process.env.BROWSER_BIN) return process.env.BROWSER_BIN;
  for (const candidate of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const resolved = childProcess.spawnSync("sh", ["-lc", `command -v ${candidate}`], {
      encoding: "utf8",
    });
    if (resolved.status === 0 && resolved.stdout.trim()) {
      return resolved.stdout.trim();
    }
  }
  return null;
}

function assertGitHubExploreUrl(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error("GITHUB_EXPLORE_URL must be an absolute GitHub Explore URL");
  }
  const host = parsed.hostname.toLowerCase().replace(/\.$/, "");
  assert(parsed.protocol === "https:", "GITHUB_EXPLORE_URL must use HTTPS");
  assert(host === "github.com", "GITHUB_EXPLORE_URL must stay on github.com");
  assert(
    ["/explore", "/topics", "/trending", "/collections"].some(
      (prefix) => parsed.pathname === prefix || parsed.pathname.startsWith(`${prefix}/`),
    ),
    "GITHUB_EXPLORE_URL must point at GitHub Explore, Topics, Trending, or Collections",
  );
}

function sourceIdentity() {
  if (cachedSourceIdentity) return cachedSourceIdentity;
  const manifestPath = process.env.AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST || "";
  if (manifestPath) {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    validateReleaseBundleManifestSource(manifestPath);
    const identity = manifest.source_identity;
    if (!identity || typeof identity !== "object" || !identity.source_hash) {
      throw new Error(`release bundle manifest does not contain source_identity: ${manifestPath}`);
    }
    cachedSourceIdentity = identity;
    return cachedSourceIdentity;
  }
  const script = [
    "import json, os, sys",
    "from pathlib import Path",
    "root = Path(os.environ['AGENT_WORKSPACE_SOURCE_ROOT'])",
    "desktop = Path(os.environ['AGENT_WORKSPACE_DESKTOP_REPO'])",
    "sys.dont_write_bytecode = True",
    "sys.path.insert(0, str(root / 'scripts'))",
    "from release_gate_audit import compute_source_identity",
    "print(json.dumps(compute_source_identity(root, desktop_repo=desktop), sort_keys=True))",
  ].join("; ");
  const completed = childProcess.spawnSync("python3", ["-c", script], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      AGENT_WORKSPACE_SOURCE_ROOT: repoRoot,
      AGENT_WORKSPACE_DESKTOP_REPO: desktopRepo,
    },
  });
  if (completed.status !== 0) {
    throw new Error(
      `failed to compute combined source identity with python3\nstdout=${completed.stdout}\nstderr=${completed.stderr}`,
    );
  }
  cachedSourceIdentity = JSON.parse(completed.stdout);
  return cachedSourceIdentity;
}

function validateReleaseBundleManifestSource(manifestPath) {
  const script = [
    "import json, os, sys",
    "from pathlib import Path",
    "root = Path(os.environ['AGENT_WORKSPACE_SOURCE_ROOT'])",
    "desktop = Path(os.environ['AGENT_WORKSPACE_DESKTOP_REPO'])",
    "manifest = Path(os.environ['AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST'])",
    "sys.dont_write_bytecode = True",
    "sys.path.insert(0, str(root / 'scripts'))",
    "from release_gate_audit import validate_bundle_manifest_source_contents",
    "validate_bundle_manifest_source_contents(json.loads(manifest.read_text(encoding='utf-8')), root=root, desktop_repo=desktop)",
  ].join("; ");
  const completed = childProcess.spawnSync("python3", ["-c", script], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      AGENT_WORKSPACE_SOURCE_ROOT: repoRoot,
      AGENT_WORKSPACE_DESKTOP_REPO: desktopRepo,
      AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST: manifestPath,
    },
  });
  if (completed.status !== 0) {
    throw new Error(
      `release bundle source bytes no longer match the manifest\nstdout=${completed.stdout}\nstderr=${completed.stderr}`,
    );
  }
}

function evidenceBoundary() {
  const mcpArgs = openViewer ? "mcp" : "mcp --headless";
  return {
    collector: "agent-workspace-linux",
    collector_script: "scripts/github_explore_dogfood_probe.js",
    repo_owned_runtime: true,
    codex_app_mcp_used: false,
    computer_use_mcp_used: false,
    codex_desktop_bridge_used: false,
    playwright_mcp_used: false,
    runtime_entrypoint: bin,
    mcp_entrypoint: `${bin} ${mcpArgs}`,
  };
}

function parseStars(value) {
  const normalized = String(value || "").trim().toLowerCase().replace(/,/g, "");
  if (!normalized) return null;
  const multiplier = normalized.endsWith("k") ? 1000 : 1;
  const number = Number(normalized.replace(/k$/, ""));
  if (!Number.isFinite(number)) return null;
  return Math.round(number * multiplier);
}

function repoBlocks(text) {
  const lines = String(text || "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const blocks = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index] !== "Trending repository") continue;
    const fullName = lines[index + 1] || "";
    if (!/^[A-Za-z0-9_.-]+\s*\/\s*[A-Za-z0-9_.-]+$/.test(fullName)) continue;
    const blockLines = [];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      if (cursor > index + 1 && lines[cursor] === "Trending repository") break;
      if (cursor > index + 1 && /^Collection recommended by GitHub$/.test(lines[cursor])) break;
      if (cursor > index + 1 && /^Trending developers$/.test(lines[cursor])) break;
      blockLines.push(lines[cursor]);
    }
    blocks.push(blockLines);
  }
  return blocks;
}

function extractDescription(block) {
  const stopWords = new Set(["Star", "Code", "Issues", "Pull requests", "Discussions", "Updated"]);
  const description = [];
  let afterNav = false;
  for (const line of block.slice(1)) {
    if (/^Star\s+/i.test(line)) continue;
    if (stopWords.has(line)) {
      if (line === "Discussions" || line === "Pull requests") afterNav = true;
      continue;
    }
    if (line === "Updated") break;
    if (/^[a-z0-9][a-z0-9+#._-]{1,40}$/i.test(line) && description.length > 0) break;
    if (afterNav || description.length > 0) {
      description.push(line);
      if (description.join(" ").length > 240) break;
    }
  }
  return description.join(" ").replace(/\s+/g, " ").trim();
}

function extractTopics(block) {
  const topics = [];
  for (const line of block) {
    if (/^[a-z0-9][a-z0-9+#._-]{1,40}$/i.test(line) && !["Code", "Issues", "Updated"].includes(line)) {
      topics.push(line.toLowerCase());
    }
  }
  return Array.from(new Set(topics)).slice(0, 12);
}

function scoreRepo(repo) {
  const haystack = `${repo.full_name} ${repo.description} ${repo.topics.join(" ")}`.toLowerCase();
  const weighted = [
    ["codex", 8],
    ["mcp", 8],
    ["agent", 7],
    ["agents", 7],
    ["developer-tools", 7],
    ["rust", 6],
    ["linux", 6],
    ["knowledge-graph", 6],
    ["codebase", 6],
    ["skills", 6],
    ["local", 5],
    ["terminal", 5],
    ["from-scratch", 4],
    ["performance", 4],
    ["security", 3],
  ];
  let score = 0;
  const matched = [];
  for (const [term, weight] of weighted) {
    if (haystack.includes(term)) {
      score += weight;
      matched.push(term);
    }
  }
  if (repo.stars_number) score += Math.min(5, Math.floor(Math.log10(repo.stars_number)));
  return { score, matched };
}

function extractRecommendations(snapshotText, limit = 3) {
  const repos = [];
  for (const block of repoBlocks(snapshotText)) {
    const fullName = block[0].replace(/\s*\/\s*/, "/");
    const starLine = block.find((line) => /^Star\s+/i.test(line)) || "";
    const stars = starLine.replace(/^Star\s+/i, "").trim() || null;
    const topics = extractTopics(block);
    const description = extractDescription(block);
    const [owner, repo] = fullName.split("/");
    const candidate = {
      full_name: fullName,
      url: `https://github.com/${owner}/${repo}`,
      stars,
      stars_number: parseStars(stars),
      description,
      topics,
    };
    const scored = scoreRepo(candidate);
    repos.push({ ...candidate, score: scored.score, matched_terms: scored.matched });
  }
  return repos
    .filter((repo) => repo.score > 0)
    .sort((a, b) => b.score - a.score || (b.stars_number || 0) - (a.stars_number || 0))
    .slice(0, limit)
    .map((repo) => ({
      full_name: repo.full_name,
      url: repo.url,
      stars: repo.stars,
      description: repo.description,
      topics: repo.topics,
      matched_terms: repo.matched_terms,
      reason: recommendationReason(repo),
    }));
}

function recommendationReason(repo) {
  const terms = repo.matched_terms.slice(0, 4).join(", ");
  if (/knowledge-graph|codebase|codex/i.test(`${repo.description} ${repo.topics.join(" ")}`)) {
    return `Strong fit for codebase-understanding and Codex-adjacent workflow work; matched ${terms}.`;
  }
  if (/from-scratch|rust|mcp|agents/i.test(`${repo.description} ${repo.topics.join(" ")}`)) {
    return `Good fit for low-level AI engineering, MCP, and agent-system study; matched ${terms}.`;
  }
  return `Likely relevant to agent tooling and developer workflow interests; matched ${terms}.`;
}

function expectSelfTestFailure(label, fn, pattern) {
  try {
    fn();
  } catch (error) {
    const message = String(error?.message || error);
    assert(pattern.test(message), `${label} failed with unexpected message: ${message}`);
    return;
  }
  throw new Error(`${label} unexpectedly passed`);
}

function runSelfTest() {
  if (process.env.AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST) {
    sourceIdentity();
  }
  assertGitHubExploreUrl("https://github.com/explore");
  assertGitHubExploreUrl("https://github.com/topics/rust");
  expectSelfTestFailure("non-GitHub URL", () => assertGitHubExploreUrl("https://example.com/explore"), /github\.com/);
  expectSelfTestFailure("local URL", () => assertGitHubExploreUrl("http://github.com/explore"), /HTTPS/);
  const sample = `
Trending repository
Lum1104 / Understand-Anything
 Star 33k
 Code
 Issues
 Pull requests
 Discussions
Graphs that teach > graphs that impress. Turn any code into an interactive knowledge graph you can explore. Works with Claude Code, Codex, Cursor, Copilot, Gemini CLI, and more.
memory
knowledge-graph
codex
developer-tools-ai-agent
Updated
 TypeScript
 Trending repository
rohitg00 / ai-engineering-from-scratch
 Star 19.5k
 Code
 Issues
 Pull requests
Learn it. Build it. Ship it for others.
rust
mcp
agents
from-scratch
Updated
 Python
 Trending repository
colbymchenry / codegraph
 Star 26.4k
 Code
 Issues
 Pull requests
Pre-indexed code knowledge graph for Claude Code, Codex, Cursor, OpenCode, and Hermes Agent - fewer tokens, fewer tool calls, 100% local
Updated
 TypeScript
`;
  const picks = extractRecommendations(sample);
  assert(picks.length === 3, `expected three recommendations, got ${JSON.stringify(picks)}`);
  assert(picks.some((repo) => repo.full_name === "Lum1104/Understand-Anything"), "knowledge graph Codex repo should be selected");
  assert(picks.some((repo) => repo.full_name === "rohitg00/ai-engineering-from-scratch"), "Rust/MCP repo should be selected");
  assert(picks.some((repo) => repo.full_name === "colbymchenry/codegraph"), "local codegraph repo should be selected");
  console.log("github explore dogfood self-test passed");
}

function startMcp(env) {
  const mcpArgs = openViewer ? ["mcp"] : ["mcp", "--headless"];
  const child = childProcess.spawn(bin, mcpArgs, {
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
        throw new Error(`invalid JSON-RPC line from MCP server: ${line}`);
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
      slot.reject(new Error(`MCP server exited before ${slot.method} response, code=${code}, signal=${signal}, stderr=${stderr}`));
    }
    pending.clear();
  });
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
  async function callTool(name, toolArgs, timeoutMs) {
    const result = await request(
      "tools/call",
      {
        name,
        arguments: toolArgs || {},
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
  return { child, request, notify, callTool, stderr: () => stderr };
}

async function runLive() {
  assertGitHubExploreUrl(targetUrl);
  const browser = resolveBrowser();
  if (!browser) {
    throw new Error("Chrome/Chromium not found; set BROWSER_BIN to run GitHub Explore dogfood");
  }
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "awl-github-explore-"));
  const configDir = path.join(tempDir, "config");
  const runtimeDir = path.join(tempDir, "runtime");
  const userDataDir = process.env.GITHUB_EXPLORE_USER_DATA_DIR || path.join(tempDir, "github-profile");
  fs.mkdirSync(configDir, { recursive: true });
  fs.mkdirSync(runtimeDir, { recursive: true });
  fs.mkdirSync(userDataDir, { recursive: true });
  fs.chmodSync(runtimeDir, 0o700);
  const env = {
    ...process.env,
    XDG_CONFIG_HOME: configDir,
    XDG_RUNTIME_DIR: runtimeDir,
  };
  const mcp = startMcp(env);
  const workspaceId = `github-explore-${process.pid}`;
  let stopped = null;
  let reportPath = null;
  let viewer = { ok: false, message: "workspace not started" };
  try {
    const initializeResult = await mcp.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "agent-workspace-linux-github-explore-dogfood", version: "0" },
    });
    mcp.notify("notifications/initialized", {});
    assert(
      /workspace_browser_snapshot/.test(String(initializeResult.instructions || "")),
      `initialize instructions should expose workspace browser controls: ${initializeResult.instructions}`,
    );
    const started = await mcp.callTool(
      "workspace_start",
      {
        id: workspaceId,
        acknowledge_hidden_workspace: true,
        purpose: "GitHub Explore repository discovery dogfood",
        width: 1280,
        height: 900,
      },
      15000,
    );
    assert(started.ok === true, `workspace_start failed: ${JSON.stringify(started)}`);
    if (openViewer) {
      viewer = await mcp.callTool(
        "workspace_open_viewer",
        {
          id: workspaceId,
          always_on_top: true,
        },
        12000,
      );
      assert(viewer.ok === true, `workspace_open_viewer failed: ${JSON.stringify(viewer)}`);
    } else {
      viewer = {
        ok: true,
        message: "viewer opening explicitly disabled",
        launch: null,
        disabled_by: "GITHUB_EXPLORE_OPEN_VIEWER=0 or GITHUB_EXPLORE_NO_VIEWER=1",
      };
    }
    const launch = await mcp.callTool(
      "workspace_launch_app",
      {
        id: workspaceId,
        name: "github-explore-browser",
        wait_window: true,
        screenshot_window: true,
        window_timeout_ms: 20000,
        command: [
          browser,
          `--user-data-dir=${userDataDir}`,
          "--no-first-run",
          "--no-default-browser-check",
          "--remote-debugging-address=127.0.0.1",
          "--remote-debugging-port=0",
          "--ozone-platform=x11",
          "--new-window",
          targetUrl,
        ],
      },
      30000,
    );
    assert(launch.ok === true, `workspace_launch_app failed: ${JSON.stringify(launch)}`);
    if (Number.isFinite(pageWaitMs) && pageWaitMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, Math.min(pageWaitMs, 30000)));
    }
    const appId = launch.apps?.[0]?.id;
    assert(appId, `workspace_launch_app should return an app id: ${JSON.stringify(launch)}`);
    const targets = await mcp.callTool(
      "workspace_browser_targets",
      {
        id: workspaceId,
        app_id: appId,
        timeout_ms: 15000,
      },
      20000,
    );
    const pageTarget =
      targets.targets?.find((target) => target.type === "page" && String(target.url || "").startsWith("https://github.com/")) ||
      targets.targets?.find((target) => target.type === "page");
    assert(pageTarget, `GitHub page target not found: ${JSON.stringify(targets)}`);
    const snapshot = await mcp.callTool(
      "workspace_browser_snapshot",
      {
        id: workspaceId,
        app_id: appId,
        target_id: pageTarget.id,
        max_text_chars: 24000,
        timeout_ms: 15000,
      },
      20000,
    );
    assert(snapshot.ok === true, `workspace_browser_snapshot failed: ${JSON.stringify(snapshot)}`);
    assert(/github\.com\/explore|github\.com\/topics|github\.com\/trending/.test(snapshot.target?.url || snapshot.page?.url || ""), `snapshot was not from GitHub Explore: ${JSON.stringify(snapshot.target || snapshot.page)}`);
    const recommendations = extractRecommendations(snapshot.page?.text || "");
    assert(recommendations.length >= 3, `GitHub Explore snapshot produced fewer than 3 relevant repositories: ${JSON.stringify(recommendations)}`);
    const events = await mcp.callTool("workspace_events", { id: workspaceId, tail: 40 }, 10000);
    if (Number.isFinite(holdSeconds) && holdSeconds > 0) {
      await new Promise((resolve) => setTimeout(resolve, Math.min(holdSeconds, 300) * 1000));
    }
    stopped = await mcp.callTool("workspace_stop", { id: workspaceId }, 15000);
    const now = new Date();
    fs.mkdirSync(reportDir, { recursive: true });
    reportPath = path.join(reportDir, `${now.toISOString().replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z")}.json`);
    const report = {
      schema: "agent-workspace-linux.github_explore_dogfood.v1",
      created_at_utc: now.toISOString(),
      source_identity: sourceIdentity(),
      evidence_boundary: evidenceBoundary(),
      mode: "workspace-github-explore",
      status: "passed",
      inputs: {
        task_intent: "github_explore_repository_discovery",
        target_url: targetUrl,
        personalized_profile_requested: Boolean(process.env.GITHUB_EXPLORE_USER_DATA_DIR),
      },
      safety_contract: {
        public_repository_discovery_only: true,
        no_host_browser_bridge: true,
        no_playwright_or_curl: true,
        no_account_mutation: true,
        raw_page_text_omitted_from_report: true,
      },
      workspace_browser: {
        status: "passed",
        control_surface: "direct_mcp_workspace_browser_devtools",
        workspace_owned_browser: true,
        host_chrome_bridge_used: false,
        coordinate_input_used: false,
        workspace_id: workspaceId,
        browser_app_id: appId,
        browser_path: browser,
        page_url: snapshot.page?.url || pageTarget.url || null,
        page_title: snapshot.page?.title || pageTarget.title || null,
        target_id: pageTarget.id,
        target_count: targets.targets?.length || 0,
        devtools_endpoint: targets.devtools_endpoint || null,
        snapshot_text_chars: snapshot.page?.text_chars || 0,
        snapshot_text_truncated: snapshot.page?.text_truncated === true,
        launch_screenshot_bytes: launch.screenshot?.bytes || 0,
        event_count: events.events?.length || 0,
        signed_in_detected: !/Sign in\s+Sign up/.test(snapshot.page?.text || ""),
        viewer,
        cleanup: stopped,
      },
      recommendations,
      recommendation_count: recommendations.length,
      artifact_digest: crypto
        .createHash("sha256")
        .update(JSON.stringify(recommendations, null, 2))
        .digest("hex"),
    };
    fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
    console.log(`github explore dogfood report: ${reportPath}`);
    console.log("github explore dogfood passed");
    for (const repo of recommendations) {
      console.log(`- ${repo.full_name}: ${repo.url}`);
    }
  } finally {
    try {
      if (!stopped) {
        await mcp.callTool("workspace_stop", { id: workspaceId }, 5000);
      }
    } catch {
      // ignore cleanup races
    }
    try {
      mcp.child.kill("SIGTERM");
    } catch {
      // ignore cleanup races
    }
    if (reportPath) {
      fs.rmSync(tempDir, { recursive: true, force: true });
    } else {
      console.error(`preserved temp dir: ${tempDir}`);
    }
  }
}

if (selfTest) {
  runSelfTest();
} else {
  runLive().catch((error) => {
    console.error(error && error.stack ? error.stack : error);
    process.exit(1);
  });
}
