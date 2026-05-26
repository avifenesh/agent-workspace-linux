#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const MANIFEST_NAME = ".agent-workspace-grocery-profile-copy.json";
const MANIFEST_SCHEMA = "agent-workspace-linux.grocery_profile_copy.v1";

const SKIP_FILE_NAMES = new Set([
  "SingletonCookie",
  "SingletonLock",
  "SingletonSocket",
  "lockfile",
]);

const SKIP_DIR_NAMES = new Set([
  "Cache",
  "Code Cache",
  "Crash Reports",
  "Crashpad",
  "DawnCache",
  "DawnWebGPUCache",
  "Extensions",
  "Extension State",
  "GPUCache",
  "GrShaderCache",
  "GraphiteDawnCache",
  "IndexedDB",
  "Local Extension Settings",
  "Media Cache",
  "Service Worker",
  "Shared Dictionary",
  "ShaderCache",
  "Web Applications",
  "component_crx_cache",
  "extensions_crx_cache",
]);

function usage(exitCode = 0) {
  const stream = exitCode === 0 ? process.stdout : process.stderr;
  stream.write(`prepare_grocery_profile_copy.js

Usage:
  scripts/prepare_grocery_profile_copy.js --source DIR --dest DIR [--profile-directory NAME] [--replace] [--dry-run]
  scripts/prepare_grocery_profile_copy.js --self-test

Copies a browser user-data directory into a disposable dogfood profile and
omits browser locks, sockets, caches, crash dumps, extension/web-app payloads,
and symlinks. The destination
gets ${MANIFEST_NAME}, which scripts/real_grocery_dogfood_probe.js uses as
machine-readable proof that the profile was prepared as a disposable copy.
When --profile-directory is set, only root-level browser files and that selected
Chrome/Chromium profile directory are copied.
`);
  process.exit(exitCode);
}

function parseArgs(argv) {
  const options = {
    source: null,
    dest: null,
    replace: false,
    dryRun: false,
    selfTest: false,
    profileDirectory: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") usage(0);
    if (arg === "--self-test") {
      options.selfTest = true;
      continue;
    }
    if (arg === "--replace") {
      options.replace = true;
      continue;
    }
    if (arg === "--dry-run") {
      options.dryRun = true;
      continue;
    }
    if (arg === "--profile-directory") {
      options.profileDirectory = normalizeProfileDirectory(argv[++index]);
      if (!options.profileDirectory) throw new Error("--profile-directory requires NAME");
      continue;
    }
    if (arg === "--source") {
      options.source = argv[++index];
      if (!options.source) throw new Error("--source requires DIR");
      continue;
    }
    if (arg === "--dest") {
      options.dest = argv[++index];
      if (!options.dest) throw new Error("--dest requires DIR");
      continue;
    }
    throw new Error(`unknown option: ${arg}`);
  }
  if (options.selfTest) return options;
  if (!options.source || !options.dest) usage(1);
  return options;
}

function normalizeProfileDirectory(value) {
  const normalized = String(value || "").trim();
  if (!normalized) return null;
  if (
    normalized === "." ||
    normalized === ".." ||
    normalized.includes("/") ||
    normalized.includes("\\") ||
    normalized.includes("\0")
  ) {
    throw new Error(`--profile-directory must be a single Chrome profile directory name, got ${value}`);
  }
  return normalized;
}

function resolveDir(value) {
  return path.resolve(value);
}

function pathInside(child, parent) {
  const relative = path.relative(parent, child);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function shouldSkipDir(name) {
  return SKIP_DIR_NAMES.has(name);
}

function shouldSkipFile(name) {
  return SKIP_FILE_NAMES.has(name) || name.endsWith("-journal") || name.startsWith(".org.chromium.");
}

function ensureSafePaths(source, dest) {
  const sourceStat = fs.statSync(source);
  if (!sourceStat.isDirectory()) {
    throw new Error(`source is not a directory: ${source}`);
  }
  if (source === dest) {
    throw new Error("--dest must be different from --source");
  }
  if (pathInside(dest, source)) {
    throw new Error("--dest must not be inside --source");
  }
}

function listDirSafe(dir) {
  try {
    return fs.readdirSync(dir, { withFileTypes: true });
  } catch (error) {
    throw new Error(`failed to read ${dir}: ${error.message}`);
  }
}

function copyTree(source, dest, options, rel = "", stats = null) {
  const result =
    stats ||
    {
      dirsCopied: 0,
      filesCopied: 0,
      bytesCopied: 0,
      skipped: [],
    };

  if (!options.dryRun) {
    fs.mkdirSync(dest, { recursive: true, mode: 0o700 });
  }
  result.dirsCopied += 1;

  for (const entry of listDirSafe(source)) {
    const sourcePath = path.join(source, entry.name);
    const destPath = path.join(dest, entry.name);
    const entryRel = rel ? path.join(rel, entry.name) : entry.name;

    if (entry.isSymbolicLink()) {
      result.skipped.push({ path: entryRel, reason: "symlink" });
      continue;
    }
    if (entry.isDirectory()) {
      if (shouldSkipDir(entry.name)) {
        result.skipped.push({ path: entryRel, reason: "cache_or_crash_dir" });
        continue;
      }
      copyTree(sourcePath, destPath, options, entryRel, result);
      continue;
    }
    if (entry.isFile()) {
      if (shouldSkipFile(entry.name)) {
        result.skipped.push({ path: entryRel, reason: "browser_lock_or_journal" });
        continue;
      }
      const stat = fs.statSync(sourcePath);
      result.filesCopied += 1;
      result.bytesCopied += stat.size;
      if (!options.dryRun) {
        fs.mkdirSync(path.dirname(destPath), { recursive: true, mode: 0o700 });
        fs.copyFileSync(sourcePath, destPath);
        fs.chmodSync(destPath, stat.mode & 0o777);
      }
      continue;
    }
    result.skipped.push({ path: entryRel, reason: "special_file" });
  }

  return result;
}

function copyRootFilesAndSelectedProfile(source, dest, options) {
  const result = {
    dirsCopied: 0,
    filesCopied: 0,
    bytesCopied: 0,
    skipped: [],
  };
  const profileDirectory = options.profileDirectory;
  const sourceProfile = path.join(source, profileDirectory);
  if (!fs.existsSync(sourceProfile) || !fs.statSync(sourceProfile).isDirectory()) {
    throw new Error(`selected profile directory does not exist: ${sourceProfile}`);
  }

  if (!options.dryRun) {
    fs.mkdirSync(dest, { recursive: true, mode: 0o700 });
  }
  result.dirsCopied += 1;

  for (const entry of listDirSafe(source)) {
    const sourcePath = path.join(source, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isSymbolicLink()) {
      result.skipped.push({ path: entry.name, reason: "symlink" });
      continue;
    }
    if (entry.isDirectory()) {
      if (entry.name === profileDirectory) {
        copyTree(sourcePath, destPath, options, entry.name, result);
      } else {
        result.skipped.push({ path: entry.name, reason: "unselected_root_dir" });
      }
      continue;
    }
    if (entry.isFile()) {
      if (shouldSkipFile(entry.name)) {
        result.skipped.push({ path: entry.name, reason: "browser_lock_or_journal" });
        continue;
      }
      const stat = fs.statSync(sourcePath);
      result.filesCopied += 1;
      result.bytesCopied += stat.size;
      if (!options.dryRun) {
        fs.mkdirSync(path.dirname(destPath), { recursive: true, mode: 0o700 });
        fs.copyFileSync(sourcePath, destPath);
        fs.chmodSync(destPath, stat.mode & 0o777);
      }
      continue;
    }
    result.skipped.push({ path: entry.name, reason: "special_file" });
  }

  return result;
}

function prepareProfileCopy(options) {
  const source = resolveDir(options.source);
  const dest = resolveDir(options.dest);
  ensureSafePaths(source, dest);

  if (fs.existsSync(dest)) {
    const entries = fs.readdirSync(dest);
    if (entries.length > 0 && !options.replace) {
      throw new Error(`destination exists and is not empty: ${dest}; pass --replace to recreate it`);
    }
    if (!options.dryRun && options.replace) {
      fs.rmSync(dest, { recursive: true, force: true });
    }
  }

  const stats = options.profileDirectory
    ? copyRootFilesAndSelectedProfile(source, dest, options)
    : copyTree(source, dest, options);
  const manifest = {
    schema: MANIFEST_SCHEMA,
    status: options.dryRun ? "dry_run" : "prepared",
    created_at_utc: new Date().toISOString(),
    source_user_data_dir: source,
    destination_user_data_dir: dest,
    profile_directory: options.profileDirectory,
    profile_scoped_copy: Boolean(options.profileDirectory),
    excludes_browser_locks_and_caches: true,
    skipped_count: stats.skipped.length,
    copied_dirs: stats.dirsCopied,
    copied_files: stats.filesCopied,
    copied_bytes: stats.bytesCopied,
    skipped: stats.skipped.slice(0, 200),
  };

  if (!options.dryRun) {
    fs.writeFileSync(path.join(dest, MANIFEST_NAME), `${JSON.stringify(manifest, null, 2)}\n`, {
      mode: 0o600,
    });
  }

  return manifest;
}

function runSelfTest() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "agent-workspace-grocery-copy-test-"));
  const source = path.join(temp, "source");
  const dest = path.join(temp, "dest");
  fs.mkdirSync(path.join(source, "Default", "Cache"), { recursive: true });
  fs.mkdirSync(path.join(source, "Default", "Code Cache"), { recursive: true });
  fs.mkdirSync(path.join(source, "Default", "Extensions", "abc", "1.0.0", "images"), { recursive: true });
  fs.mkdirSync(path.join(source, "Default", "IndexedDB", "https_example.com_0.indexeddb.leveldb"), { recursive: true });
  fs.mkdirSync(path.join(source, "Default", "Web Applications", "Manifest Resources", "app", "Icons"), { recursive: true });
  fs.mkdirSync(path.join(source, "Profile 1", "Cache"), { recursive: true });
  fs.mkdirSync(path.join(source, "Profile 1", "Extensions", "def", "1.0.0", "images"), { recursive: true });
  fs.mkdirSync(path.join(source, "Profile 1", "Service Worker"), { recursive: true });
  fs.writeFileSync(path.join(source, "Default", "Preferences"), "{}\n");
  fs.writeFileSync(path.join(source, "Default", "Cookies"), "cookie-db\n");
  fs.writeFileSync(path.join(source, "Profile 1", "Preferences"), "{}\n");
  fs.writeFileSync(path.join(source, "Profile 1", "Cookies"), "profile-cookie-db\n");
  fs.writeFileSync(path.join(source, "Local State"), "{}\n");
  fs.writeFileSync(path.join(source, "SingletonLock"), "lock\n");
  fs.writeFileSync(path.join(source, "Default", "Cache", "cached"), "cache\n");
  fs.writeFileSync(path.join(source, "Default", "Code Cache", "compiled"), "compiled\n");
  fs.writeFileSync(path.join(source, "Default", "Extensions", "abc", "1.0.0", "images", "icon.png"), "png\n");
  fs.writeFileSync(
    path.join(source, "Default", "IndexedDB", "https_example.com_0.indexeddb.leveldb", "000001.ldb"),
    "large site storage\n",
  );
  fs.writeFileSync(
    path.join(source, "Default", "Web Applications", "Manifest Resources", "app", "Icons", "icon.png"),
    "png\n",
  );
  fs.writeFileSync(path.join(source, "Profile 1", "Extensions", "def", "1.0.0", "images", "icon.png"), "png\n");
  fs.writeFileSync(path.join(source, "Profile 1", "Service Worker", "script.js"), "worker\n");

  try {
    const manifest = prepareProfileCopy({ source, dest, replace: false, dryRun: false });
    assertSelfTest(manifest.status === "prepared", "manifest should be prepared");
    assertSelfTest(fs.existsSync(path.join(dest, "Default", "Preferences")), "preferences copied");
    assertSelfTest(fs.existsSync(path.join(dest, "Default", "Cookies")), "cookies copied");
    assertSelfTest(!fs.existsSync(path.join(dest, "SingletonLock")), "lock skipped");
    assertSelfTest(!fs.existsSync(path.join(dest, "Default", "Cache")), "cache dir skipped");
    assertSelfTest(!fs.existsSync(path.join(dest, "Default", "Code Cache")), "code cache skipped");
    assertSelfTest(!fs.existsSync(path.join(dest, "Default", "Extensions")), "extensions skipped");
    assertSelfTest(!fs.existsSync(path.join(dest, "Default", "IndexedDB")), "indexeddb skipped");
    assertSelfTest(!fs.existsSync(path.join(dest, "Default", "Web Applications")), "web apps skipped");
    assertSelfTest(fs.existsSync(path.join(dest, MANIFEST_NAME)), "manifest written");
    const scopedDest = path.join(temp, "scoped-dest");
    const scoped = prepareProfileCopy({
      source,
      dest: scopedDest,
      replace: false,
      dryRun: false,
      profileDirectory: "Profile 1",
    });
    assertSelfTest(scoped.profile_directory === "Profile 1", "scoped manifest records selected profile");
    assertSelfTest(scoped.profile_scoped_copy === true, "scoped manifest marks scoped copy");
    assertSelfTest(fs.existsSync(path.join(scopedDest, "Local State")), "root Local State copied");
    assertSelfTest(fs.existsSync(path.join(scopedDest, "Profile 1", "Preferences")), "selected profile copied");
    assertSelfTest(!fs.existsSync(path.join(scopedDest, "Default", "Preferences")), "unselected profile skipped");
    assertSelfTest(!fs.existsSync(path.join(scopedDest, "Profile 1", "Cache")), "selected profile cache skipped");
    assertSelfTest(!fs.existsSync(path.join(scopedDest, "Profile 1", "Extensions")), "selected profile extensions skipped");
    assertSelfTest(!fs.existsSync(path.join(scopedDest, "Profile 1", "Service Worker")), "selected profile service worker skipped");
    console.log("grocery profile copy self-test passed");
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

function assertSelfTest(condition, message) {
  if (!condition) {
    throw new Error(`self-test failed: ${message}`);
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.selfTest) {
    runSelfTest();
    return;
  }
  const manifest = prepareProfileCopy(options);
  console.log(JSON.stringify(manifest, null, 2));
}

try {
  main();
} catch (error) {
  console.error(error && error.stack ? error.stack : error);
  process.exitCode = 1;
}
