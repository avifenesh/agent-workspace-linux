#!/usr/bin/env node
// postinstall.js — downloads the matching prebuilt binary from GitHub Releases.
//
// Invoked automatically by npm after `npm install -g @agent-sh/agent-workspace-linux`.
// If --ignore-scripts was used (e.g. pnpm default, some CI setups) this script
// will NOT run; the binary won't be present and the launcher will print a clear
// error at runtime. To recover, re-run installation with scripts enabled or
// invoke this file directly: `node node_modules/@agent-sh/agent-workspace-linux/scripts/postinstall.js`
//
// Checksum verification is not wired in because the release workflow contract
// does not publish .sha256 sidecar files. If that changes, add a step here that
// downloads <asset>.sha256 and verifies it against the tmp file before rename.

"use strict";

const https = require("https");
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

// ── Platform / arch guard ────────────────────────────────────────────────────

if (process.platform !== "linux") {
  console.error(
    `agent-workspace-linux: unsupported platform "${process.platform}". ` +
      "This package only works on Linux."
  );
  process.exit(1);
}

const ARCH_MAP = {
  x64: "x86_64-unknown-linux-gnu",
  arm64: "aarch64-unknown-linux-gnu",
};

const rustTarget = ARCH_MAP[process.arch];
if (!rustTarget) {
  console.error(
    `agent-workspace-linux: unsupported CPU architecture "${process.arch}". ` +
      `Supported: ${Object.keys(ARCH_MAP).join(", ")}.`
  );
  process.exit(1);
}

// ── Paths ────────────────────────────────────────────────────────────────────

const pkg = require("../package.json");
const version = pkg.version; // e.g. "0.1.0"
const assetName = `agent-workspace-linux-${rustTarget}`;
const downloadUrl =
  `https://github.com/agent-sh/agent-workspace-linux/releases/download/` +
  `v${version}/${assetName}`;

const binDir = path.join(__dirname, "..", "bin");
const destPath = path.join(binDir, "agent-workspace-linux");
const tmpPath = destPath + ".tmp";

// ── Download helper (follows redirects, max 5 hops) ─────────────────────────

/**
 * Download `url` to `tmpFile`, following up to `maxRedirects` 3xx responses.
 * Resolves when the file is fully written, rejects on error or bad status.
 */
function download(url, tmpFile, maxRedirects = 5) {
  return new Promise((resolve, reject) => {
    function get(currentUrl, hopsLeft) {
      https
        .get(currentUrl, { headers: { "User-Agent": "node-fetch/postinstall" } }, (res) => {
          const { statusCode, headers } = res;

          // Follow redirects (GitHub releases always redirect to S3).
          if (statusCode >= 300 && statusCode < 400 && headers.location) {
            if (hopsLeft === 0) {
              res.resume();
              return reject(new Error(`Too many redirects downloading ${url}`));
            }
            res.resume(); // drain and ignore body
            return get(headers.location, hopsLeft - 1);
          }

          if (statusCode !== 200) {
            res.resume();
            return reject(
              new Error(
                `Failed to download ${url}: HTTP ${statusCode}. ` +
                  "Check that the release exists and the version in package.json matches."
              )
            );
          }

          const out = fs.createWriteStream(tmpFile);
          res.pipe(out);
          out.on("finish", () => out.close(resolve));
          out.on("error", (err) => {
            fs.unlink(tmpFile, () => {}); // best-effort cleanup
            reject(err);
          });
          res.on("error", (err) => {
            fs.unlink(tmpFile, () => {});
            reject(err);
          });
        })
        .on("error", (err) => {
          fs.unlink(tmpFile, () => {});
          reject(err);
        });
    }

    get(url, maxRedirects);
  });
}

// ── Main ─────────────────────────────────────────────────────────────────────

(async () => {
  // Ensure bin/ directory exists (it should, tracked by git via .gitkeep, but
  // be defensive for edge-case installs).
  fs.mkdirSync(binDir, { recursive: true });

  // Remove stale tmp file from a previous interrupted install.
  try {
    fs.unlinkSync(tmpPath);
  } catch (_) {}

  console.log(
    `agent-workspace-linux: downloading ${assetName} v${version}…`
  );
  console.log(`  URL: ${downloadUrl}`);

  try {
    await download(downloadUrl, tmpPath);
  } catch (err) {
    // Clean up in case the file was partially created.
    try {
      fs.unlinkSync(tmpPath);
    } catch (_) {}
    // The download error text embeds the request URL (built from
    // package.json's version + the target triple). Log a fixed message rather
    // than interpolating that value, to avoid a log-injection sink; the URL
    // itself is already printed above for debugging.
    console.error(
      "\nagent-workspace-linux: download failed. Verify the release exists " +
        "for your platform and that the version in package.json matches " +
        "(see the URL logged above)."
    );
    process.exit(1);
  }

  // Atomic rename: avoids leaving a half-written binary.
  try {
    fs.renameSync(tmpPath, destPath);
  } catch (err) {
    console.error(
      `agent-workspace-linux: could not move binary into place — ${err.message}`
    );
    process.exit(1);
  }

  // Mark executable.
  try {
    fs.chmodSync(destPath, 0o755);
  } catch (err) {
    console.error(
      `agent-workspace-linux: chmod failed — ${err.message}. ` +
        `Try: chmod 755 ${destPath}`
    );
    process.exit(1);
  }

  console.log(`agent-workspace-linux: binary installed at ${destPath}`);
})();
