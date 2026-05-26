#!/usr/bin/env node
"use strict";

const childProcess = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..");
const desktopRepo = process.env.CODEX_DESKTOP_LINUX_REPO || path.join(repoRoot, "..", "codex-desktop-linux");
const cliArgs = process.argv.slice(2);
const selfTest = cliArgs.includes("--self-test");
const printCartDraftStepsTemplate = cliArgs.includes("--print-cart-draft-steps-template");
const preflightRealGrocery = cliArgs.includes("--preflight-real-grocery");
const validateCartDraftStepsIndex = cliArgs.indexOf("--validate-cart-draft-steps");
const bin =
  process.env.AGENT_WORKSPACE_BIN ||
  process.env.BIN ||
  path.join(repoRoot, "target", "debug", "agent-workspace-linux");
let cachedSourceIdentity = null;

if (
  !selfTest &&
  !printCartDraftStepsTemplate &&
  !preflightRealGrocery &&
  validateCartDraftStepsIndex === -1 &&
  !fs.existsSync(bin)
) {
  throw new Error(`agent-workspace-linux binary not found at ${bin}; run cargo build first`);
}

const realMode = process.env.REAL_GROCERY_DOGFOOD === "1";
const targetUrl = process.env.GROCERY_TARGET_URL || "https://example-grocery.test";
const shoppingList = process.env.GROCERY_SHOPPING_LIST || "milk 2L, eggs 12, bananas 1kg";
const fulfillment = process.env.GROCERY_FULFILLMENT || "delivery";
const substitutionPolicy = process.env.GROCERY_SUBSTITUTION_POLICY || "ask before substitutions";
const budget = process.env.GROCERY_BUDGET || "$50";
const cartApproved = process.env.CART_MUTATION_APPROVED === "1";
const finalCartReviewed = process.env.FINAL_CART_REVIEWED === "1";
const realWorldApproved =
  process.env.REAL_WORLD_ACTION_APPROVED === "1" || process.env.CHECKOUT_APPROVED === "1";
const holdSeconds = Number(process.env.REAL_GROCERY_HOLD_SECONDS || "0");
const preserveRealGroceryWorkspace = process.env.REAL_GROCERY_PRESERVE_WORKSPACE === "1";
const openViewer = process.env.REAL_GROCERY_OPEN_VIEWER === "1";
const cartDraftStepsPath =
  process.env.GROCERY_CART_DRAFT_STEPS_JSON || process.env.REAL_GROCERY_CART_DRAFT_STEPS_JSON || "";
const realBrowserInteractionMode = normalizeRealBrowserInteractionMode(
  process.env.REAL_GROCERY_INTERACTION_MODE ||
    process.env.REAL_GROCERY_BROWSER_MODE ||
    (cartDraftStepsPath ? "cart-draft-approved" : "observe-only"),
);
const workspaceInputEventKinds = new Set([
  "click",
  "click_window",
  "move_pointer",
  "move_pointer_window",
  "drag",
  "drag_window",
  "scroll",
  "scroll_window",
  "key",
  "key_window",
  "type_text",
  "type_window",
  "set_clipboard",
  "paste_text",
  "paste_window",
  "focus_window",
  "focus_matching_window",
  "close_window",
  "close_matching_window",
  "move_window",
  "resize_window",
  "raise_window",
  "minimize_window",
  "show_window",
  "kill_app",
]);
const cartDraftStepActions = new Set([
  "observe",
  "wait_window",
  "key_window",
  "type_window",
  "paste_window",
  "click_window",
  "scroll_window",
]);
const cartDraftInputStepActions = new Set([
  "key_window",
  "type_window",
  "paste_window",
  "click_window",
  "scroll_window",
]);
const allowedCartDraftInputEventKinds = new Set([
  "key_window",
  "type_window",
  "paste_window",
  "click_window",
  "scroll_window",
]);
const forbiddenCartDraftIntent =
  /\b(checkout|place\s+order|submit\s+order|complete\s+order|buy\s+now|pay(?:ment)?|card|cvv|account|password|sign\s*up|create\s+account|log\s*in|login|subscribe)\b/i;

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "agent-workspace-real-grocery-"));
const configDir = path.join(tempDir, "config");
const runtimeDir = path.join(tempDir, "runtime");
const fallbackUserDataDir = path.join(tempDir, "browser-data");
fs.mkdirSync(configDir, { recursive: true });
fs.mkdirSync(runtimeDir, { recursive: true });
fs.mkdirSync(fallbackUserDataDir, { recursive: true });

const userDataDir = process.env.GROCERY_USER_DATA_DIR || fallbackUserDataDir;
const groceryProfileDirectory = normalizeProfileDirectory(
  process.env.GROCERY_PROFILE_DIRECTORY || process.env.REAL_GROCERY_PROFILE_DIRECTORY || "",
);
const profileCopyManifestPath =
  process.env.GROCERY_PROFILE_COPY_MANIFEST ||
  path.join(userDataDir, ".agent-workspace-grocery-profile-copy.json");
const reportDir = process.env.REPORT_DIR || path.join(repoRoot, "target", "real-grocery-dogfood");
fs.mkdirSync(reportDir, { recursive: true });
const reportPath = path.join(reportDir, `${new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z")}.json`);

let mcpChild = null;
let nextId = 1;
let stdoutBuffer = "";
let stderr = "";
const pending = new Map();

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function normalizeRealBrowserInteractionMode(value) {
  const normalized = String(value || "").trim().toLowerCase().replace(/_/g, "-");
  if (["observe", "observe-only", "readonly", "read-only"].includes(normalized)) {
    return "observe-only";
  }
  if (["cart-draft", "cart-draft-approved", "draft-cart", "draft-cart-approved"].includes(normalized)) {
    return "cart-draft-approved";
  }
  throw new Error(
    `REAL_GROCERY_INTERACTION_MODE must be observe-only or cart-draft-approved, got ${value}`,
  );
}

function normalizeProfileDirectory(value) {
  const normalized = String(value || "").trim();
  if (!normalized) return null;
  assert(
    normalized !== "." &&
      normalized !== ".." &&
      !normalized.includes("/") &&
      !normalized.includes("\\") &&
      !normalized.includes("\0"),
    `GROCERY_PROFILE_DIRECTORY must be a single Chrome profile directory name, got ${value}`,
  );
  return normalized;
}

function actionOf(step) {
  return String(step?.action || step?.kind || "").trim().toLowerCase().replace(/-/g, "_");
}

function safeStepText(value) {
  return typeof value === "string" ? value.trim() : "";
}

function assertNoForbiddenCartDraftIntent(value, context) {
  if (!value) return;
  assert(
    !forbiddenCartDraftIntent.test(String(value)),
    `cart-draft step ${context} mentions checkout/payment/account mutation; use a separate approval path for that`,
  );
}

function readCartDraftStepsFromPath(stepPath) {
  assert(stepPath, "cart-draft-approved real grocery mode requires GROCERY_CART_DRAFT_STEPS_JSON");
  const parsed = JSON.parse(fs.readFileSync(stepPath, "utf8"));
  const steps = Array.isArray(parsed) ? parsed : parsed?.steps;
  assert(Array.isArray(steps), "cart draft steps JSON must contain an array or an object with steps");
  assert(steps.length > 0, "cart draft steps JSON must contain at least one step");
  assert(steps.length <= 80, "cart draft steps JSON may contain at most 80 steps");
  return steps;
}

function readCartDraftSteps() {
  return readCartDraftStepsFromPath(cartDraftStepsPath);
}

function fileSha256(filePath) {
  return crypto.createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function cartDraftStepsEvidence(stepPath, steps, validation) {
  const stat = fs.statSync(stepPath);
  return {
    path: stepPath,
    sha256: fileSha256(stepPath),
    size_bytes: stat.size,
    step_count: steps.length,
    input_step_count: validation.inputStepCount,
    cart_mutation_step_count: validation.cartMutationStepCount,
    summaries: validation.summaries,
  };
}

function cartDraftStepsTemplate() {
  return {
    schema: "agent-workspace-linux.grocery_cart_draft_steps.v1",
    notes: [
      "Use only workspace-local browser input needed to draft the cart.",
      "Do not include checkout, payment, login, account creation, or order submission steps.",
      "Mark at least one input step with cart_mutation=true when it drafts/adds items to the cart.",
    ],
    steps: [
      {
        action: "key_window",
        key: "ctrl+l",
        safety_label: "Focus the grocery site address/search field before drafting the cart.",
      },
      {
        action: "paste_window",
        text: "milk 2L, eggs 12, bananas 1kg",
        safety_label: "Enter only the approved shopping-list text for cart drafting.",
      },
      {
        action: "key_window",
        key: "Return",
        cart_mutation: true,
        expected_effect: "cart_draft",
        safety_label: "Confirm the approved draft-cart action only; stop at the cart state.",
      },
      {
        action: "observe",
        safety_label: "Collect evidence that the cart draft is visible and checkout remains blocked.",
      },
    ],
  };
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
  const template = cartDraftStepsTemplate();
  const validated = validateCartDraftSteps(template.steps);
  assert(validated.inputStepCount === 3, "template should include three input steps");
  assert(validated.cartMutationStepCount === 1, "template should include one cart mutation step");
  expectSelfTestFailure(
    "checkout label rejection",
    () =>
      validateCartDraftSteps([
        {
          action: "click_window",
          x: 100,
          y: 120,
          cart_mutation: true,
          safety_label: "Click checkout to place order",
        },
      ]),
    /checkout\/payment\/account mutation/,
  );
  expectSelfTestFailure(
    "missing cart mutation rejection",
    () =>
      validateCartDraftSteps([
        {
          action: "paste_window",
          text: "milk 2L",
          safety_label: "Paste an approved shopping-list item.",
        },
      ]),
    /at least one step marked cart_mutation/,
  );
  expectSelfTestFailure(
    "unsupported action rejection",
    () =>
      validateCartDraftSteps([
        {
          action: "kill_app",
          cart_mutation: true,
          safety_label: "Stop the browser after drafting.",
        },
      ]),
    /unsupported cart-draft action/,
  );
  expectSelfTestFailure(
    "failed cart step result rejection",
    () => assertCartDraftStepResultOk(2, "paste_window", { ok: false, message: "window not found" }),
    /cart-draft step 2 \(paste_window\) failed/,
  );
  const audit = summarizeWorkspaceInputEvents(
    [
      { kind: "paste_window", sequence: 1 },
      { kind: "key_window", sequence: 2 },
      { kind: "kill_app", sequence: 3 },
    ],
    allowedCartDraftInputEventKinds,
    {
      expectedInputStepCount: 4,
      eventsTailRequested: 120,
      minimumEventsTailRequired: 120,
      eventsSinceSequence: 0,
    },
  );
  assert(audit.input_event_count === 3, "input audit should count all input events");
  assert(audit.input_event_count_covers_expected === false, "input audit should prove when declared input is missing");
  assert(audit.unexpected_input_event_count === 1, "input audit should flag disallowed input events");
  assert(audit.unexpected_input_event_kinds.includes("kill_app"), "input audit should name the disallowed kind");
  assert(viewerEntryIsAlive({ alive: true }) === true, "viewer registry should treat alive=true as live");
  assert(
    viewerEntryIsAlive({ running: true }) === true,
    "viewer registry should accept legacy running=true rows as live",
  );
  assert(viewerEntryIsAlive({ alive: false, running: false }) === false, "stale viewer rows should not count");
  const redactedSnapshot = privacyPreservingPageSnapshot({
    title: "Release Grocery",
    url: "https://www.kroger.com/cart",
    text: "name, address, phone, and cart details must not be persisted",
    text_chars: 64,
    text_truncated: false,
  });
  assert(redactedSnapshot.raw_text_omitted === true, "real-grocery snapshot evidence should omit raw page text");
  assert(!Object.hasOwn(redactedSnapshot, "text"), "real-grocery snapshot evidence must not store raw page text");
  assert(!Object.hasOwn(redactedSnapshot, "text_excerpt"), "real-grocery snapshot evidence must not store raw page excerpts");
  assert(redactedSnapshot.text_chars === 64, "real-grocery snapshot evidence should keep text length metadata");
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "agent-workspace-grocery-preflight-test-"));
  try {
    const sourceProfile = path.join(temp, "source-profile");
    const userDataDir = path.join(temp, "copied-profile");
    const stepsPath = path.join(temp, "cart-draft-steps.json");
    fs.mkdirSync(sourceProfile, { recursive: true });
    fs.mkdirSync(path.join(userDataDir, "Profile 1"), { recursive: true });
    fs.mkdirSync(userDataDir, { recursive: true });
    fs.writeFileSync(
      path.join(userDataDir, ".agent-workspace-grocery-profile-copy.json"),
      JSON.stringify(
        {
          schema: "agent-workspace-linux.grocery_profile_copy.v1",
          status: "prepared",
          created_at_utc: new Date().toISOString(),
          source_user_data_dir: sourceProfile,
          destination_user_data_dir: userDataDir,
          profile_directory: "Profile 1",
          profile_scoped_copy: true,
          excludes_browser_locks_and_caches: true,
        },
        null,
        2,
      ),
    );
    fs.writeFileSync(stepsPath, JSON.stringify(template, null, 2));
    const expectedStepsSha256 = fileSha256(stepsPath);
    const preflight = childProcess.spawnSync(process.execPath, [__filename, "--preflight-real-grocery"], {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        REAL_GROCERY_DOGFOOD: "1",
        REAL_GROCERY_INTERACTION_MODE: "cart-draft-approved",
        CART_MUTATION_APPROVED: "1",
        FINAL_CART_REVIEWED: "1",
        GROCERY_TARGET_URL: "https://www.kroger.com",
        GROCERY_USER_DATA_DIR: userDataDir,
        GROCERY_PROFILE_DIRECTORY: "Profile 1",
        GROCERY_PROFILE_IS_DISPOSABLE_COPY: "1",
        GROCERY_CART_DRAFT_STEPS_JSON: stepsPath,
        BROWSER_BIN: process.execPath,
        CHECKOUT_APPROVED: "",
        REAL_WORLD_ACTION_APPROVED: "",
      },
    });
    assert(
      preflight.status === 0,
      `real grocery preflight self-test failed\nstdout=${preflight.stdout}\nstderr=${preflight.stderr}`,
    );
    const preflightJson = JSON.parse(preflight.stdout);
    assert(preflightJson.status === "passed", "preflight self-test should pass");
    assert(preflightJson.profile_directory === "Profile 1", "preflight should report selected Chrome profile directory");
    assert(
      preflightJson.profile_copy_manifest.profile_directory === "Profile 1",
      "profile-copy manifest summary should carry selected Chrome profile directory",
    );
    assert(preflightJson.cart_draft_steps.cart_mutation_step_count === 1, "preflight should validate cart mutation steps");
    assert(preflightJson.cart_draft_steps.sha256 === expectedStepsSha256, "preflight should bind the approved step file hash");
    assert(preflightJson.cart_draft_steps.size_bytes > 0, "preflight should bind the approved step file size");
    assert(preflightJson.checkout_or_real_world_approval_refused === true, "preflight should refuse checkout approval");
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

function stepTimeoutMs(step, fallback = 5000) {
  const value = Number(step?.timeout_ms ?? step?.timeoutMs ?? fallback);
  assert(Number.isFinite(value) && value >= 0 && value <= 60000, "step timeout_ms must be between 0 and 60000");
  return Math.trunc(value);
}

function assertCoordinate(value, label) {
  assert(Number.isInteger(value), `${label} must be an integer`);
  assert(value >= -2000 && value <= 20000, `${label} is outside the expected screen-coordinate range`);
}

function validateCartDraftSteps(steps) {
  let inputStepCount = 0;
  let cartMutationStepCount = 0;
  const summaries = [];
  for (const [index, step] of steps.entries()) {
    assert(step && typeof step === "object" && !Array.isArray(step), `step ${index + 1} must be an object`);
    const action = actionOf(step);
    assert(cartDraftStepActions.has(action), `step ${index + 1} uses unsupported cart-draft action ${action}`);
    const safetyLabel = safeStepText(step.safety_label || step.safetyLabel || step.note || step.description);
    const isInputStep = cartDraftInputStepActions.has(action);
    const isCartMutationStep =
      step.cart_mutation === true ||
      step.cartMutation === true ||
      ["cart_draft", "draft_cart_changes", "add_to_cart"].includes(
        String(step.expected_effect || step.expectedEffect || "").trim().toLowerCase(),
      );
    if (isInputStep) {
      inputStepCount += 1;
      assert(safetyLabel, `cart-draft input step ${index + 1} requires safety_label or note`);
      assertNoForbiddenCartDraftIntent(safetyLabel, `${index + 1} safety_label`);
      assertNoForbiddenCartDraftIntent(step.text, `${index + 1} text`);
      assertNoForbiddenCartDraftIntent(step.key, `${index + 1} key`);
      assertNoForbiddenCartDraftIntent(step.direction, `${index + 1} direction`);
    }
    if (isCartMutationStep) {
      cartMutationStepCount += 1;
      assert(isInputStep, `cart mutation marker on step ${index + 1} must be attached to an input action`);
      assert(safetyLabel, `cart mutation step ${index + 1} requires a safety label`);
    }
    if (["type_window", "paste_window"].includes(action)) {
      const text = String(step.text ?? "");
      assert(text.length > 0, `step ${index + 1} ${action} requires text`);
      assert(text.length <= 1000, `step ${index + 1} text is too long for release evidence`);
    }
    if (action === "key_window") {
      assert(safeStepText(step.key), `step ${index + 1} key_window requires key`);
    }
    if (action === "click_window") {
      assertCoordinate(step.x, `step ${index + 1} x`);
      assertCoordinate(step.y, `step ${index + 1} y`);
    }
    if (action === "scroll_window") {
      assertCoordinate(step.x, `step ${index + 1} x`);
      assertCoordinate(step.y, `step ${index + 1} y`);
      assert(["up", "down", "left", "right"].includes(String(step.direction || "")), `step ${index + 1} scroll_window requires direction up/down/left/right`);
    }
    summaries.push({
      index: index + 1,
      action,
      safety_label: safetyLabel || null,
      cart_mutation: isCartMutationStep,
      text_bytes: typeof step.text === "string" ? Buffer.byteLength(step.text) : null,
    });
  }
  assert(inputStepCount > 0, "cart-draft-approved mode requires at least one declared input step");
  assert(cartMutationStepCount > 0, "cart-draft-approved mode requires at least one step marked cart_mutation=true or expected_effect=cart_draft");
  return { inputStepCount, cartMutationStepCount, summaries };
}

function startMcp() {
  mcpChild = childProcess.spawn(bin, ["mcp", "--headless"], {
    cwd: repoRoot,
    env: {
      ...process.env,
      XDG_CONFIG_HOME: configDir,
      XDG_RUNTIME_DIR: runtimeDir,
    },
    stdio: ["pipe", "pipe", "pipe"],
  });

  mcpChild.stderr.on("data", (chunk) => {
    stderr += String(chunk);
  });

  mcpChild.stdout.on("data", (chunk) => {
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
        failMcp(`invalid JSON-RPC line from MCP server: ${line}`);
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

  mcpChild.on("exit", (code, signal) => {
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
}

function failMcp(message) {
  try {
    mcpChild?.kill("SIGTERM");
  } catch {
    // ignore cleanup races
  }
  throw new Error(message);
}

function request(method, params, timeoutMs = 5000, label = method) {
  const id = nextId++;
  mcpChild.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`timed out waiting for ${label}`));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer, method: label });
  });
}

function notify(method, params) {
  mcpChild.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
}

async function callTool(name, args, timeoutMs) {
  const result = await request(
    "tools/call",
    { name, arguments: args || {} },
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

function boundary(plan, id) {
  return plan.task_context?.action_boundaries?.find((entry) => entry.id === id);
}

function resolveExecutableCandidate(candidate) {
  const value = String(candidate || "").trim();
  if (!value) return null;
  if (value.includes("/") || path.isAbsolute(value)) {
    try {
      fs.accessSync(value, fs.constants.X_OK);
      return value;
    } catch {
      return null;
    }
  }
  const resolved = childProcess.spawnSync("sh", ["-lc", `command -v ${JSON.stringify(value)}`], {
    encoding: "utf8",
  });
  return resolved.status === 0 && resolved.stdout.trim() ? resolved.stdout.trim() : null;
}

function findBrowser() {
  if (process.env.BROWSER_BIN) return resolveExecutableCandidate(process.env.BROWSER_BIN);
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

function isPublicIpv4(hostname) {
  const parts = hostname.split(".").map((part) => Number(part));
  if (parts.length !== 4 || parts.some((part) => !Number.isInteger(part) || part < 0 || part > 255)) {
    return false;
  }
  const [a, b, c] = parts;
  if (a === 0 || a === 10 || a === 127 || a >= 224) return false;
  if (a === 100 && b >= 64 && b <= 127) return false;
  if (a === 169 && b === 254) return false;
  if (a === 172 && b >= 16 && b <= 31) return false;
  if (a === 192 && (b === 0 || b === 168)) return false;
  if (a === 198 && (b === 18 || b === 19)) return false;
  if (a === 192 && b === 0 && c === 2) return false;
  if (a === 198 && b === 51 && c === 100) return false;
  if (a === 203 && b === 0 && c === 113) return false;
  return true;
}

function isPublicIp(hostname) {
  const ipVersion = net.isIP(hostname);
  if (ipVersion === 4) return isPublicIpv4(hostname);
  if (ipVersion === 6) {
    const lower = hostname.toLowerCase();
    return (
      lower !== "::" &&
      lower !== "::1" &&
      !lower.startsWith("fc") &&
      !lower.startsWith("fd") &&
      !lower.startsWith("fe80") &&
      !lower.startsWith("2001:db8")
    );
  }
  return true;
}

function assertRealGroceryTargetUrl(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error("REAL_GROCERY_DOGFOOD=1 requires GROCERY_TARGET_URL to be an absolute URL");
  }
  const hostname = parsed.hostname.toLowerCase().replace(/\.$/, "");
  assert(parsed.protocol === "https:", "REAL_GROCERY_DOGFOOD=1 requires an HTTPS grocery URL");
  assert(hostname, "REAL_GROCERY_DOGFOOD=1 requires GROCERY_TARGET_URL to include a hostname");
  assert(
    !["localhost", "example.com", "example.net", "example.org"].includes(hostname) &&
      !hostname.endsWith(".localhost") &&
      !hostname.endsWith(".local") &&
      !hostname.endsWith(".test") &&
      !hostname.endsWith(".invalid") &&
      !hostname.endsWith(".example") &&
      isPublicIp(hostname),
    "REAL_GROCERY_DOGFOOD=1 requires a real non-local grocery site, not a localhost, reserved, or private-network URL",
  );
}

function sourceIdentity() {
  if (cachedSourceIdentity) {
    return cachedSourceIdentity;
  }
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
  return {
    collector: "agent-workspace-linux",
    collector_script: "scripts/real_grocery_dogfood_probe.js",
    repo_owned_runtime: true,
    codex_app_mcp_used: false,
    computer_use_mcp_used: false,
    codex_desktop_bridge_used: false,
    playwright_mcp_used: false,
    runtime_entrypoint: bin,
    mcp_entrypoint: `${bin} mcp --headless`,
  };
}

function runCli(args, options = {}) {
  const { allowFailure = false, ...spawnOptions } = options;
  const completed = childProcess.spawnSync(bin, args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      XDG_CONFIG_HOME: configDir,
      XDG_RUNTIME_DIR: runtimeDir,
    },
    maxBuffer: 8 * 1024 * 1024,
    ...spawnOptions,
  });
  if (completed.status !== 0 && !allowFailure) {
    throw new Error(
      `${bin} ${args.join(" ")} failed with ${completed.status}\nstdout=${completed.stdout}\nstderr=${completed.stderr}`,
    );
  }
  if (completed.status !== 0) {
    return {
      ok: false,
      status: completed.status,
      signal: completed.signal || null,
      stdout: completed.stdout || "",
      stderr: completed.stderr || "",
    };
  }
  return completed.stdout.trim() ? JSON.parse(completed.stdout) : null;
}

function spawnVisibleViewer(workspaceId) {
  const env = {
    ...process.env,
    AGENT_WORKSPACE_VIEWER_BACKEND: process.env.AGENT_WORKSPACE_VIEWER_BACKEND || "x11",
  };
  const child = childProcess.spawn(
    bin,
    ["viewer", "--id", workspaceId, "--exit-when-workspace-gone"],
    {
      cwd: repoRoot,
      env,
      detached: true,
      stdio: "ignore",
    },
  );
  child.unref();
  return {
    status: "started",
    pid: child.pid,
    command: [bin, "viewer", "--id", workspaceId, "--exit-when-workspace-gone"],
    backend: env.AGENT_WORKSPACE_VIEWER_BACKEND,
    always_on_top: false,
    repo_owned_runtime: true,
  };
}

async function waitForRegisteredViewer(workspaceId, timeoutMs = 5000) {
  const started = Date.now();
  let last = null;
  while (Date.now() - started < timeoutMs) {
    last = runCli(["viewer", "list"], { allowFailure: true });
    const viewers = Array.isArray(last?.viewers) ? last.viewers : [];
    const match = viewers.find((viewer) => viewer?.id === workspaceId && viewerEntryIsAlive(viewer));
    if (match) {
      return {
        status: "registered",
        viewer: match,
        list: last,
      };
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return {
    status: "not_registered",
    list: last,
  };
}

function viewerEntryIsAlive(viewer) {
  return viewer?.alive === true || viewer?.running === true;
}

function browserAppIdFromLaunch(launch) {
  const apps = Array.isArray(launch?.apps) ? launch.apps : [];
  return apps.find((app) => app?.running && app?.id)?.id || apps.find((app) => app?.id)?.id || "real-grocery-browser";
}

function isLoopbackDevToolsEndpoint(value) {
  try {
    const parsed = new URL(String(value || ""));
    return parsed.protocol === "http:" && ["127.0.0.1", "localhost", "::1", "[::1]"].includes(parsed.hostname);
  } catch {
    return false;
  }
}

function endpointPort(value) {
  try {
    const parsed = new URL(String(value || ""));
    const parsedPort = Number(parsed.port);
    return Number.isInteger(parsedPort) && parsedPort > 0 ? parsedPort : null;
  } catch {
    return null;
  }
}

async function captureWorkspaceBrowserDevtools(workspaceId, browserAppId, userDataDir, expectedUrl) {
  const workspaceTargets = runCli([
    "workspace",
    "browser-targets",
    "--id",
    workspaceId,
    "--app",
    browserAppId,
    "--user-data-dir",
    userDataDir,
    "--timeout-ms",
    "10000",
  ]);
  assert(workspaceTargets?.ok === true, `workspace browser-targets failed: ${JSON.stringify(workspaceTargets)}`);
  assert(
    workspaceTargets.app_id === browserAppId,
    `workspace browser-targets returned app ${workspaceTargets.app_id}, expected ${browserAppId}`,
  );
  assert(
    isLoopbackDevToolsEndpoint(workspaceTargets.devtools_endpoint),
    `workspace browser-targets returned a non-loopback DevTools endpoint: ${workspaceTargets.devtools_endpoint}`,
  );
  const targets = Array.isArray(workspaceTargets.targets) ? workspaceTargets.targets : [];
  const expectedHost = (() => {
    try {
      return new URL(expectedUrl).hostname.toLowerCase();
    } catch {
      return "";
    }
  })();
  const page =
    targets.find(
      (target) =>
        target?.type === "page" &&
        target?.webSocketDebuggerUrl &&
        expectedHost &&
        String(target.url || "").toLowerCase().includes(expectedHost),
    ) ||
    targets.find((target) => target?.type === "page" && target?.webSocketDebuggerUrl);
  assert(page, `Chrome DevTools target list did not expose a page target: ${JSON.stringify(targets)}`);
  const snapshotResult = runCli([
    "workspace",
    "browser-snapshot",
    "--id",
    workspaceId,
    "--app",
    browserAppId,
    "--user-data-dir",
    userDataDir,
    "--target",
    page.id,
    "--max-text-chars",
    "4000",
    "--timeout-ms",
    "10000",
  ]);
  assert(
    snapshotResult?.ok === true,
    `workspace browser-snapshot failed: ${JSON.stringify(snapshotResult)}`,
  );
  const snapshotPage = snapshotResult.page || {};
  const snapshot = privacyPreservingPageSnapshot(snapshotPage);
  return {
    status: "passed",
    control_surface: "workspace_chrome_devtools",
    workspace_owned_browser: true,
    host_chrome_bridge_used: false,
    coordinate_input_used: false,
    endpoint: workspaceTargets.devtools_endpoint,
    port: endpointPort(workspaceTargets.devtools_endpoint),
    devtools_active_port_file: workspaceTargets.devtools_active_port_path,
    browser_path: workspaceTargets.browser_path || null,
    target_count: targets.length,
    target: {
      id: page.id,
      type: page.type,
      title: page.title,
      url: page.url,
    },
    workspace_browser_targets: {
      ok: workspaceTargets.ok,
      message: workspaceTargets.message || null,
      app_id: workspaceTargets.app_id,
      app_pid: workspaceTargets.app_pid ?? null,
      workspace_user_data_dir: workspaceTargets.workspace_user_data_dir || null,
      host_user_data_dir: workspaceTargets.host_user_data_dir || null,
      devtools_endpoint: workspaceTargets.devtools_endpoint,
      devtools_active_port_path: workspaceTargets.devtools_active_port_path || null,
      target_count: targets.length,
      selected_page_target: {
        id: page.id,
        type: page.type,
        title: page.title,
        url: page.url,
        webSocketDebuggerUrl: page.webSocketDebuggerUrl || null,
      },
      warnings: Array.isArray(workspaceTargets.warnings) ? workspaceTargets.warnings : [],
    },
    workspace_browser_snapshot: {
      ok: snapshotResult.ok,
      message: snapshotResult.message || null,
      app_id: snapshotResult.app_id || null,
      target_id: snapshotResult.target?.id || null,
      page_url: snapshotPage.url || null,
      page_title: snapshotPage.title || null,
      text_chars: snapshotPage.text_chars ?? null,
      text_truncated: snapshotPage.text_truncated ?? null,
      warnings: Array.isArray(snapshotResult.warnings) ? snapshotResult.warnings : [],
    },
    page_snapshot: snapshot,
  };
}

function privacyPreservingPageSnapshot(snapshotPage) {
  return {
    source: "workspace_browser_snapshot",
    title: snapshotPage.title || "",
    url: snapshotPage.url || "",
    text_chars: snapshotPage.text_chars ?? null,
    text_truncated: snapshotPage.text_truncated ?? null,
    raw_text_omitted: true,
    raw_text_omission_reason: "release evidence avoids storing logged-in grocery page text",
  };
}

function summarizeStepResult(result) {
  return {
    ok: result?.ok ?? null,
    message: result?.message || null,
    active_window_title: result?.active_window?.title || null,
    target_window_title: result?.target_window?.title || null,
    window_count: Array.isArray(result?.windows) ? result.windows.length : null,
    screenshot_bytes: result?.screenshot?.bytes || null,
  };
}

function assertCartDraftStepResultOk(index, action, result) {
  assert(
    result?.ok === true,
    `cart-draft step ${index} (${action}) failed or did not return ok=true: ${JSON.stringify(
      summarizeStepResult(result),
    )}`,
  );
}

function maxEventSequence(events) {
  const sequences = (Array.isArray(events) ? events : [])
    .map((event) => Number(event?.sequence))
    .filter((sequence) => Number.isInteger(sequence) && sequence >= 0);
  return sequences.length > 0 ? Math.max(...sequences) : 0;
}

function cartDraftMinimumEventTail(validation, steps) {
  const stepCount = Array.isArray(steps) ? steps.length : 0;
  const inputStepCount = Number.isInteger(validation?.inputStepCount) ? validation.inputStepCount : 0;
  return Math.max(120, stepCount * 4 + inputStepCount * 6 + 40);
}

function cartDraftEventsTail(validation, steps) {
  return Math.min(1000, cartDraftMinimumEventTail(validation, steps));
}

function baseWindowArgs(workspaceId, appId, step) {
  const args = ["workspace", actionOf(step).replace(/_/g, "-"), "--id", workspaceId, "--app", appId];
  const timeoutMs = stepTimeoutMs(step);
  if (timeoutMs > 0) args.push("--timeout-ms", String(timeoutMs));
  return args;
}

function executeCartDraftStep(workspaceId, appId, step) {
  const action = actionOf(step);
  if (action === "observe") {
    return runCli(["workspace", "observe", "--id", workspaceId, "--screenshot", "--events", "--events-tail", "30"]);
  }
  if (action === "wait_window") {
    const args = ["workspace", "wait-window", "--id", workspaceId, "--app", appId];
    const timeoutMs = stepTimeoutMs(step, 10000);
    if (timeoutMs > 0) args.push("--timeout-ms", String(timeoutMs));
    if (safeStepText(step.title)) args.push("--title", safeStepText(step.title));
    if (safeStepText(step.class)) args.push("--class", safeStepText(step.class));
    return runCli(args);
  }
  if (action === "key_window") {
    return runCli([...baseWindowArgs(workspaceId, appId, step), safeStepText(step.key)]);
  }
  if (action === "type_window") {
    return runCli([...baseWindowArgs(workspaceId, appId, step), String(step.text)]);
  }
  if (action === "paste_window") {
    const args = baseWindowArgs(workspaceId, appId, step);
    if (safeStepText(step.key)) args.push("--key", safeStepText(step.key));
    return runCli([...args, String(step.text)]);
  }
  if (action === "click_window") {
    const args = baseWindowArgs(workspaceId, appId, step);
    if (Number.isInteger(step.button)) args.push("--button", String(step.button));
    if (Number.isInteger(step.count)) args.push("--count", String(step.count));
    return runCli([...args, String(step.x), String(step.y)]);
  }
  if (action === "scroll_window") {
    const args = baseWindowArgs(workspaceId, appId, step);
    if (Number.isInteger(step.amount)) args.push("--amount", String(step.amount));
    return runCli([...args, String(step.x), String(step.y), String(step.direction)]);
  }
  throw new Error(`unsupported cart-draft action ${action}`);
}

function executeCartDraftSteps(workspaceId, appId, steps, validation, stepsEvidence) {
  const executed = [];
  for (const [index, step] of steps.entries()) {
    const summary = validation.summaries[index];
    const startedAt = new Date().toISOString();
    const result = executeCartDraftStep(workspaceId, appId, step);
    assertCartDraftStepResultOk(index + 1, summary.action, result);
    executed.push({
      ...summary,
      started_at_utc: startedAt,
      result: summarizeStepResult(result),
    });
  }
  return {
    status: "passed",
    mode: "cart-draft-approved",
    steps_path: cartDraftStepsPath,
    steps_sha256: stepsEvidence.sha256,
    steps_size_bytes: stepsEvidence.size_bytes,
    step_count: steps.length,
    input_step_count: validation.inputStepCount,
    cart_mutation_step_count: validation.cartMutationStepCount,
    forbidden_step_count: 0,
    cart_mutation_approval_confirmed: cartApproved,
    final_cart_reviewed_confirmed: finalCartReviewed,
    checkout_or_real_world_approval_refused: !realWorldApproved,
    executed_steps: executed,
  };
}

async function verifyPlans() {
  const initializeResult = await request("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "agent-workspace-linux-real-grocery-probe", version: "0" },
  });
  notify("notifications/initialized", {});
  assert(/configured=false/.test(String(initializeResult.instructions || "")), "probe MCP should use clean/default permission mode");

  const baseArgs = {
    intent: "real grocery dogfood",
    user_data_dir: userDataDir,
    target_url: targetUrl,
    shopping_list: shoppingList,
    fulfillment,
    substitution_policy: substitutionPolicy,
    budget,
  };

  const unapprovedPlan = await callTool("mcp_task_plan", baseArgs);
  const unapprovedCart = boundary(unapprovedPlan, "draft_cart_changes");
  const unapprovedCheckout = boundary(unapprovedPlan, "checkout_order_or_account_change");
  assert(unapprovedCart?.approval_required === true, "cart mutation must be approval-gated before cart approval");
  assert(unapprovedCart?.approved === false, "cart mutation must not be approved by default");
  assert(
    unapprovedCart?.missing_approvals?.includes("explicit_cart_mutation_approval"),
    "cart mutation should name explicit_cart_mutation_approval as missing",
  );
  assert(unapprovedCheckout?.approval_required === true, "checkout/account changes must be approval-gated");
  assert(unapprovedCheckout?.approved === false, "checkout/account changes must not be approved by default");
  assert(
    unapprovedCheckout?.missing_approvals?.includes("final_cart_review") &&
      unapprovedCheckout?.missing_approvals?.includes("explicit_checkout_approval"),
    "checkout should require both final cart review and explicit checkout approval by default",
  );

  const cartOnlyPlan = await callTool("mcp_task_plan", {
    ...baseArgs,
    cart_mutation_approved: true,
    final_cart_reviewed: true,
    real_world_action_approved: false,
  });
  const cartOnlyBoundary = boundary(cartOnlyPlan, "draft_cart_changes");
  const checkoutBoundary = boundary(cartOnlyPlan, "checkout_order_or_account_change");
  assert(cartOnlyBoundary?.approved === true, "cart mutation should become approved after explicit cart approval");
  assert((cartOnlyBoundary?.missing_approvals || []).length === 0, "approved cart mutation should not miss approvals");
  assert(checkoutBoundary?.approved === false, "checkout must stay blocked without real-world approval");
  assert(
    checkoutBoundary?.missing_approvals?.includes("explicit_checkout_approval") &&
      !checkoutBoundary?.missing_approvals?.includes("final_cart_review"),
    "checkout should only miss explicit checkout approval after final cart review is recorded",
  );

  return { unapprovedPlan, cartOnlyPlan };
}

function verifyRealModeGuardrails() {
  assert(realMode, "internal guard misuse");
  assertRealGroceryTargetUrl(targetUrl);
  assert(process.env.GROCERY_USER_DATA_DIR, "REAL_GROCERY_DOGFOOD=1 requires GROCERY_USER_DATA_DIR");
  assert(fs.existsSync(userDataDir), `GROCERY_USER_DATA_DIR does not exist: ${userDataDir}`);
  assert(
    process.env.GROCERY_PROFILE_IS_DISPOSABLE_COPY === "1",
    "REAL_GROCERY_DOGFOOD=1 requires GROCERY_PROFILE_IS_DISPOSABLE_COPY=1; do not point this at a primary browser profile",
  );
  return verifyProfileCopyManifest();
}

function verifyProfileCopyManifest() {
  assert(
    fs.existsSync(profileCopyManifestPath),
    `REAL_GROCERY_DOGFOOD=1 requires a profile copy manifest at ${profileCopyManifestPath}; run scripts/prepare_grocery_profile_copy.js first`,
  );
  const manifest = JSON.parse(fs.readFileSync(profileCopyManifestPath, "utf8"));
  assert(
    manifest.schema === "agent-workspace-linux.grocery_profile_copy.v1",
    "profile copy manifest has the wrong schema",
  );
  assert(manifest.status === "prepared", "profile copy manifest must have status=prepared");
  assert(
    path.resolve(manifest.destination_user_data_dir || "") === path.resolve(userDataDir),
    "profile copy manifest destination_user_data_dir must match GROCERY_USER_DATA_DIR",
  );
  assert(
    path.resolve(manifest.source_user_data_dir || "") !== path.resolve(userDataDir),
    "profile copy manifest source_user_data_dir must differ from GROCERY_USER_DATA_DIR",
  );
  assert(
    manifest.excludes_browser_locks_and_caches === true,
    "profile copy manifest must confirm browser lock/cache exclusion",
  );
  let profileDirectoryPath = null;
  if (groceryProfileDirectory) {
    assert(
      manifest.profile_directory === groceryProfileDirectory,
      `profile copy manifest profile_directory must match requested profile directory ${groceryProfileDirectory}`,
    );
    assert(
      manifest.profile_scoped_copy === true,
      "profile copy manifest must confirm a profile-scoped copy when REAL_GROCERY_PROFILE_DIRECTORY is set",
    );
    profileDirectoryPath = path.join(userDataDir, groceryProfileDirectory);
    assert(
      fs.existsSync(profileDirectoryPath) && fs.statSync(profileDirectoryPath).isDirectory(),
      `requested Chrome profile directory does not exist in disposable copy: ${profileDirectoryPath}`,
    );
  }
  assert(!realWorldApproved, "this probe refuses checkout/order/account approval; unset CHECKOUT_APPROVED and REAL_WORLD_ACTION_APPROVED");
  if (realBrowserInteractionMode === "cart-draft-approved") {
    assert(
      cartApproved,
      "cart-draft-approved real grocery mode requires CART_MUTATION_APPROVED=1",
    );
    assert(
      finalCartReviewed,
      "cart-draft-approved real grocery mode requires FINAL_CART_REVIEWED=1",
    );
    validateCartDraftSteps(readCartDraftSteps());
  }
  assert(holdSeconds >= 0 && Number.isFinite(holdSeconds), "REAL_GROCERY_HOLD_SECONDS must be a non-negative number");
  return {
    path: profileCopyManifestPath,
    schema: manifest.schema,
    status: manifest.status,
    created_at_utc: manifest.created_at_utc || null,
    destination_user_data_dir: manifest.destination_user_data_dir || null,
    profile_directory: groceryProfileDirectory,
    profile_directory_path: profileDirectoryPath,
    excludes_browser_locks_and_caches: manifest.excludes_browser_locks_and_caches === true,
  };
}

function realGroceryPreflightReport() {
  assert(
    realMode,
    "--preflight-real-grocery requires REAL_GROCERY_DOGFOOD=1 so the checked environment matches the real run",
  );
  assert(
    realBrowserInteractionMode === "cart-draft-approved",
    "--preflight-real-grocery is for the release cart-draft gate; set REAL_GROCERY_INTERACTION_MODE=cart-draft-approved",
  );
  const profileCopyManifest = verifyRealModeGuardrails();
  const browser = findBrowser();
  assert(
    browser,
    "real grocery preflight requires an executable BROWSER_BIN or installed Chrome/Chromium",
  );
  const steps = readCartDraftSteps();
  const validation = validateCartDraftSteps(steps);
  const stepsEvidence = cartDraftStepsEvidence(cartDraftStepsPath, steps, validation);
  return {
    schema: "agent-workspace-linux.real_grocery_dogfood_preflight.v1",
    status: "passed",
    created_at_utc: new Date().toISOString(),
    source_identity: sourceIdentity(),
    target_url: targetUrl,
    browser,
    user_data_dir: userDataDir,
    profile_directory: groceryProfileDirectory,
    profile_copy_manifest: profileCopyManifest,
    cart_draft_steps: stepsEvidence,
    approvals: {
      cart_mutation_approved: cartApproved,
      final_cart_reviewed: finalCartReviewed,
      checkout_or_real_world_approved: realWorldApproved,
    },
    checkout_or_real_world_approval_refused: !realWorldApproved,
  };
}

function summarizeWorkspaceInputEvents(events, allowedInputKinds = null, options = {}) {
  const safeEvents = Array.isArray(events) ? events : [];
  const inputEvents = safeEvents.filter((event) => workspaceInputEventKinds.has(event?.kind));
  const unexpectedInputEvents =
    allowedInputKinds == null ? [] : inputEvents.filter((event) => !allowedInputKinds.has(event?.kind));
  const expectedInputStepCount = Number.isInteger(options.expectedInputStepCount)
    ? options.expectedInputStepCount
    : null;
  const eventsTailRequested = Number.isInteger(options.eventsTailRequested)
    ? options.eventsTailRequested
    : null;
  const minimumEventsTailRequired = Number.isInteger(options.minimumEventsTailRequired)
    ? options.minimumEventsTailRequired
    : null;
  const eventsSinceSequence = Number.isInteger(options.eventsSinceSequence)
    ? options.eventsSinceSequence
    : null;
  return {
    checked: Array.isArray(events),
    event_scope: eventsSinceSequence == null ? "tail" : "since_sequence",
    events_since_sequence: eventsSinceSequence,
    events_tail_requested: eventsTailRequested,
    minimum_events_tail_required: minimumEventsTailRequired,
    total_events: safeEvents.length,
    expected_input_step_count: expectedInputStepCount,
    input_event_count: inputEvents.length,
    input_event_count_covers_expected:
      expectedInputStepCount == null ? null : inputEvents.length >= expectedInputStepCount,
    input_event_kinds: [...new Set(inputEvents.map((event) => event.kind))].sort(),
    input_event_sequences: inputEvents.slice(0, 20).map((event) => event.sequence),
    allowed_input_event_kinds:
      allowedInputKinds == null ? null : [...allowedInputKinds].sort(),
    unexpected_input_event_count: unexpectedInputEvents.length,
    unexpected_input_event_kinds: [...new Set(unexpectedInputEvents.map((event) => event.kind))].sort(),
    unexpected_input_event_sequences: unexpectedInputEvents.slice(0, 20).map((event) => event.sequence),
  };
}

async function maybeLaunchRealBrowser(report) {
  if (!realMode) {
    report.real_browser = {
      status: "skipped",
      reason: "set REAL_GROCERY_DOGFOOD=1 with an explicit disposable copied browser profile to open a real grocery site",
    };
    return;
  }

  const profileCopyManifest = verifyRealModeGuardrails();
  const browser = findBrowser();
  assert(browser, "REAL_GROCERY_DOGFOOD=1 requires Chrome/Chromium or BROWSER_BIN");
  let cartDraftSteps = null;
  let cartDraftValidation = null;
  let cartDraftEvidence = null;
  if (realBrowserInteractionMode === "cart-draft-approved") {
    cartDraftSteps = readCartDraftSteps();
    cartDraftValidation = validateCartDraftSteps(cartDraftSteps);
    cartDraftEvidence = cartDraftStepsEvidence(cartDraftStepsPath, cartDraftSteps, cartDraftValidation);
  }

  const workspaceId = process.env.REAL_GROCERY_WORKSPACE_ID || `real-grocery-dogfood-${process.pid}`;
  report.real_browser = {
    status: "started",
    interaction_mode: realBrowserInteractionMode,
    workspace_id: workspaceId,
    browser,
    target_url: targetUrl,
    user_data_dir: userDataDir,
    profile_directory: groceryProfileDirectory,
    checkout_approval_refused: true,
    profile_copy_manifest_valid: true,
    profile_copy_manifest: profileCopyManifest,
    cart_draft_steps: cartDraftEvidence,
  };

  try {
    report.real_browser.start = runCli([
      "workspace",
      "start",
      "--ack-hidden-workspace",
      "--id",
      workspaceId,
      "--purpose",
      "Real grocery dogfood probe",
    ]);
    if (openViewer) {
      report.real_browser.viewer = spawnVisibleViewer(workspaceId);
      report.real_browser.viewer_registration = await waitForRegisteredViewer(workspaceId);
    }
    const browserArgs = [
      "workspace",
      "launch",
      "--id",
      workspaceId,
      "--name",
      "real-grocery-browser",
      "--wait-window",
      "--screenshot-window",
      "--window-timeout-ms",
      "15000",
      "--",
      browser,
      `--user-data-dir=${userDataDir}`,
    ];
    if (groceryProfileDirectory) {
      browserArgs.push(`--profile-directory=${groceryProfileDirectory}`);
    }
    browserArgs.push(
      "--no-first-run",
      "--no-default-browser-check",
      "--remote-debugging-address=127.0.0.1",
      "--remote-debugging-port=0",
      "--ozone-platform=x11",
      "--new-window",
      targetUrl,
    );
    report.real_browser.launch = runCli(browserArgs);
    const browserAppId = browserAppIdFromLaunch(report.real_browser.launch);
    report.real_browser.browser_app_id = browserAppId;
    report.real_browser.chrome_devtools = await captureWorkspaceBrowserDevtools(
      workspaceId,
      browserAppId,
      userDataDir,
      targetUrl,
    );
    const eventBaseline = runCli([
      "workspace",
      "observe",
      "--id",
      workspaceId,
      "--events",
      "--events-tail",
      "1",
    ]);
    const eventsSinceSequence = maxEventSequence(eventBaseline?.events);
    report.real_browser.event_baseline = {
      events_tail_requested: 1,
      event_count: Array.isArray(eventBaseline?.events) ? eventBaseline.events.length : null,
      sequence: eventsSinceSequence,
    };
    let expectedInputStepCount = null;
    let eventsTailRequested = 80;
    let minimumEventsTailRequired = 80;
    if (realBrowserInteractionMode === "cart-draft-approved") {
      expectedInputStepCount = cartDraftValidation.inputStepCount;
      minimumEventsTailRequired = cartDraftMinimumEventTail(cartDraftValidation, cartDraftSteps);
      eventsTailRequested = cartDraftEventsTail(cartDraftValidation, cartDraftSteps);
      report.real_browser.cart_draft_interaction = executeCartDraftSteps(
        workspaceId,
        browserAppId,
        cartDraftSteps,
        cartDraftValidation,
        cartDraftEvidence,
      );
    }
    report.real_browser.observe = runCli([
      "workspace",
      "observe",
      "--id",
      workspaceId,
      "--screenshot",
      "--events",
      "--events-tail",
      String(eventsTailRequested),
      "--events-since",
      String(eventsSinceSequence),
    ]);
    report.real_browser.workspace_input_audit = summarizeWorkspaceInputEvents(
      report.real_browser.observe?.events,
      realBrowserInteractionMode === "cart-draft-approved" ? allowedCartDraftInputEventKinds : null,
      {
        expectedInputStepCount,
        eventsTailRequested,
        minimumEventsTailRequired,
        eventsSinceSequence,
      },
    );
    assert(
      report.real_browser.workspace_input_audit.checked,
      "real grocery observe response must include workspace events for the interaction audit",
    );
    if (realBrowserInteractionMode === "observe-only") {
      assert(
        report.real_browser.workspace_input_audit.input_event_count === 0,
        `real grocery probe is observe-only but saw workspace input events: ${report.real_browser.workspace_input_audit.input_event_kinds.join(", ")}`,
      );
    } else {
      assert(
        report.real_browser.cart_draft_interaction?.status === "passed",
        "cart-draft-approved mode must execute declared cart-draft steps",
      );
      assert(
        report.real_browser.cart_draft_interaction?.cart_mutation_step_count > 0,
        "cart-draft-approved mode must include at least one cart mutation step",
      );
      assert(
        report.real_browser.workspace_input_audit.input_event_count > 0,
        "cart-draft-approved mode must prove workspace input happened",
      );
      assert(
        report.real_browser.workspace_input_audit.input_event_count_covers_expected === true,
        "cart-draft-approved mode must prove every declared input step produced workspace input evidence",
      );
      assert(
        report.real_browser.workspace_input_audit.unexpected_input_event_count === 0,
        `cart-draft-approved mode saw unexpected workspace input events: ${report.real_browser.workspace_input_audit.unexpected_input_event_kinds.join(", ")}`,
      );
    }
    if (holdSeconds > 0) {
      report.real_browser.hold_seconds = holdSeconds;
      await new Promise((resolve) => setTimeout(resolve, holdSeconds * 1000));
    }
    report.real_browser.status = "passed";
  } finally {
    try {
      report.real_browser.stop = runCli(["workspace", "stop", "--id", workspaceId]);
    } catch (error) {
      report.real_browser.stop_error = String(error && error.stack ? error.stack : error);
    }
    if (preserveRealGroceryWorkspace) {
      report.real_browser.workspace_preserved_for_debug = true;
    } else {
      report.real_browser.cleanup = runCli(["workspace", "cleanup", "--id", workspaceId]);
    }
  }
}

async function main() {
  startMcp();
  const report = {
    schema: "agent-workspace-linux.real_grocery_dogfood_probe.v1",
    created_at_utc: new Date().toISOString(),
    source_identity: sourceIdentity(),
    evidence_boundary: evidenceBoundary(),
    mode: realMode ? "real-browser" : "plan-only",
    inputs: {
      target_url: targetUrl,
      shopping_list: shoppingList,
      fulfillment,
      substitution_policy: substitutionPolicy,
      budget,
      user_data_dir: userDataDir,
      profile_directory: groceryProfileDirectory,
      profile_is_disposable_copy_env: process.env.GROCERY_PROFILE_IS_DISPOSABLE_COPY === "1",
      profile_copy_manifest_path: profileCopyManifestPath,
      real_browser_interaction_mode: realBrowserInteractionMode,
    cart_draft_steps_path: cartDraftStepsPath || null,
    real_grocery_preserve_workspace_env: preserveRealGroceryWorkspace,
    real_grocery_open_viewer_env: openViewer,
    cart_mutation_approved_env: cartApproved,
      final_cart_reviewed_env: finalCartReviewed,
      checkout_or_real_world_approved_env: realWorldApproved,
    },
    safety_contract: {
      refuses_checkout_or_real_world_approval: true,
      real_browser_requires_disposable_profile_copy: true,
      real_browser_interaction_mode: realBrowserInteractionMode,
      cart_draft_requires_explicit_approval: true,
      checkout_order_or_account_change_blocked: true,
      real_browser_sends_no_workspace_input: realBrowserInteractionMode === "observe-only",
      real_browser_observes_only: realBrowserInteractionMode === "observe-only",
    real_browser_allows_only_declared_cart_draft_input:
        realBrowserInteractionMode === "cart-draft-approved",
      real_browser_cleans_workspace_runtime: !preserveRealGroceryWorkspace,
      visible_viewer_requested: openViewer,
    },
  };

  const plans = await verifyPlans();
  report.plan_assertions = {
    status: "passed",
    unapproved_next_boundary: plans.unapprovedPlan.approval_summary?.next_boundary || null,
    cart_only_next_boundary: plans.cartOnlyPlan.approval_summary?.next_boundary || null,
    checkout_still_blocked_after_cart_approval: true,
  };

  await maybeLaunchRealBrowser(report);

  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`real grocery dogfood report: ${reportPath}`);
  console.log(`real grocery dogfood probe passed (${report.mode})`);
}

async function entrypoint() {
  if (
    selfTest ||
    preflightRealGrocery ||
    (!printCartDraftStepsTemplate && validateCartDraftStepsIndex === -1)
  ) {
    sourceIdentity();
  }
  if (selfTest) {
    runSelfTest();
    console.log("real grocery dogfood probe self-test passed");
    return;
  }
  if (printCartDraftStepsTemplate) {
    console.log(JSON.stringify(cartDraftStepsTemplate(), null, 2));
    return;
  }
  if (validateCartDraftStepsIndex !== -1) {
    const stepPath = cliArgs[validateCartDraftStepsIndex + 1];
    assert(stepPath, "--validate-cart-draft-steps requires a JSON path");
    const steps = readCartDraftStepsFromPath(stepPath);
    const validation = validateCartDraftSteps(steps);
    console.log(
      JSON.stringify(
        {
          schema: "agent-workspace-linux.grocery_cart_draft_steps_validation.v1",
          status: "passed",
          path: stepPath,
          step_count: steps.length,
          input_step_count: validation.inputStepCount,
          cart_mutation_step_count: validation.cartMutationStepCount,
          summaries: validation.summaries,
        },
        null,
        2,
      ),
    );
    return;
  }
  if (preflightRealGrocery) {
    console.log(JSON.stringify(realGroceryPreflightReport(), null, 2));
    return;
  }
  await main();
}

entrypoint()
  .catch((error) => {
    console.error(error && error.stack ? error.stack : error);
    process.exitCode = 1;
  })
  .finally(() => {
    try {
      mcpChild?.kill("SIGTERM");
    } catch {
      // ignore cleanup races
    }
    if (!realMode || preflightRealGrocery) {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });
