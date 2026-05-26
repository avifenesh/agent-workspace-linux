"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function httpJson(url, options = {}) {
  const parsed = new URL(url);
  if (parsed.protocol !== "http:") {
    throw new Error(`Chrome DevTools endpoint must be http:, got ${url}`);
  }
  return new Promise((resolve, reject) => {
    const request = http.request(
      {
        hostname: parsed.hostname,
        port: parsed.port || 80,
        path: `${parsed.pathname}${parsed.search}`,
        method: options.method || "GET",
        timeout: options.timeoutMs || 5000,
      },
      (response) => {
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => {
          body += chunk;
        });
        response.on("end", () => {
          if (response.statusCode < 200 || response.statusCode >= 300) {
            reject(new Error(`DevTools HTTP ${response.statusCode} for ${url}: ${body}`));
            return;
          }
          try {
            resolve(body ? JSON.parse(body) : null);
          } catch (error) {
            reject(new Error(`DevTools HTTP response was not JSON for ${url}: ${error.message}`));
          }
        });
      },
    );
    request.on("timeout", () => {
      request.destroy(new Error(`timed out reading Chrome DevTools endpoint ${url}`));
    });
    request.on("error", reject);
    request.end();
  });
}

function normalizeEndpoint(endpoint) {
  const normalized = String(endpoint || "").trim().replace(/\/+$/, "");
  if (!normalized) {
    throw new Error("Chrome DevTools endpoint is required");
  }
  const parsed = new URL(normalized);
  if (parsed.protocol !== "http:") {
    throw new Error(`Chrome DevTools endpoint must be http:, got ${endpoint}`);
  }
  if (!["127.0.0.1", "localhost", "::1", "[::1]"].includes(parsed.hostname)) {
    throw new Error(`Chrome DevTools endpoint must be loopback, got ${endpoint}`);
  }
  return normalized;
}

async function listTargets(endpoint) {
  const base = normalizeEndpoint(endpoint);
  return httpJson(`${base}/json/list`);
}

async function createTarget(endpoint, targetUrl) {
  const base = normalizeEndpoint(endpoint);
  const encoded = encodeURIComponent(targetUrl);
  try {
    return await httpJson(`${base}/json/new?${encoded}`, { method: "PUT" });
  } catch (error) {
    if (!/HTTP 405|HTTP 501/.test(String(error.message || error))) {
      throw error;
    }
    return httpJson(`${base}/json/new?${encoded}`, { method: "GET" });
  }
}

async function waitForDevToolsEndpoint(userDataDir, timeoutMs = 10000) {
  const file = path.join(userDataDir, "DevToolsActivePort");
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const lines = fs.readFileSync(file, "utf8").trim().split(/\r?\n/);
      const port = Number(lines[0]);
      if (Number.isInteger(port) && port > 0) {
        return {
          endpoint: `http://127.0.0.1:${port}`,
          port,
          browser_path: lines[1] || null,
          file,
        };
      }
      lastError = new Error(`DevToolsActivePort did not contain a valid port: ${lines[0]}`);
    } catch (error) {
      lastError = error;
    }
    await sleep(100);
  }
  throw new Error(`timed out waiting for ${file}: ${lastError && lastError.message}`);
}

class CdpConnection {
  constructor(socket, leftover = Buffer.alloc(0)) {
    this.socket = socket;
    this.buffer = leftover;
    this.nextId = 1;
    this.pending = new Map();
    this.closed = false;

    socket.on("data", (chunk) => this.onData(chunk));
    socket.on("error", (error) => this.rejectAll(error));
    socket.on("close", () => {
      this.closed = true;
      this.rejectAll(new Error("Chrome DevTools WebSocket closed"));
    });
    if (leftover.length > 0) {
      this.onData(Buffer.alloc(0));
    }
  }

  static connect(webSocketDebuggerUrl, timeoutMs = 5000) {
    const parsed = new URL(webSocketDebuggerUrl);
    if (parsed.protocol !== "ws:") {
      throw new Error(`Chrome DevTools WebSocket must be ws:, got ${webSocketDebuggerUrl}`);
    }
    if (!["127.0.0.1", "localhost", "::1", "[::1]"].includes(parsed.hostname)) {
      throw new Error(`Chrome DevTools WebSocket must be loopback, got ${webSocketDebuggerUrl}`);
    }
    const key = crypto.randomBytes(16).toString("base64");
    const expectedAccept = crypto
      .createHash("sha1")
      .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
      .digest("base64");
    const port = Number(parsed.port || 80);
    const request = [
      `GET ${parsed.pathname}${parsed.search} HTTP/1.1`,
      `Host: ${parsed.hostname}:${port}`,
      "Upgrade: websocket",
      "Connection: Upgrade",
      `Sec-WebSocket-Key: ${key}`,
      "Sec-WebSocket-Version: 13",
      "\r\n",
    ].join("\r\n");

    return new Promise((resolve, reject) => {
      const socket = net.connect({ host: parsed.hostname, port });
      let handshake = Buffer.alloc(0);
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new Error(`timed out connecting to ${webSocketDebuggerUrl}`));
      }, timeoutMs);

      socket.once("error", (error) => {
        clearTimeout(timer);
        reject(error);
      });
      socket.on("data", function onHandshake(chunk) {
        handshake = Buffer.concat([handshake, chunk]);
        const headerEnd = handshake.indexOf("\r\n\r\n");
        if (headerEnd === -1) return;
        socket.off("data", onHandshake);
        clearTimeout(timer);
        const header = handshake.slice(0, headerEnd).toString("utf8");
        const leftover = handshake.slice(headerEnd + 4);
        if (!/^HTTP\/1\.1 101\b/.test(header) && !/^HTTP\/1\.0 101\b/.test(header)) {
          socket.destroy();
          reject(new Error(`Chrome DevTools WebSocket handshake failed: ${header}`));
          return;
        }
        const acceptLine = header
          .split(/\r?\n/)
          .find((line) => line.toLowerCase().startsWith("sec-websocket-accept:"));
        if (!acceptLine || acceptLine.split(":").slice(1).join(":").trim() !== expectedAccept) {
          socket.destroy();
          reject(new Error("Chrome DevTools WebSocket handshake returned an invalid accept key"));
          return;
        }
        resolve(new CdpConnection(socket, leftover));
      });
      socket.write(request);
    });
  }

  request(method, params = {}, timeoutMs = 5000) {
    if (this.closed) {
      return Promise.reject(new Error("Chrome DevTools WebSocket is closed"));
    }
    const id = this.nextId++;
    this.sendJson({ id, method, params });
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`timed out waiting for CDP ${method}`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer, method });
    });
  }

  sendJson(message) {
    this.sendFrame(Buffer.from(JSON.stringify(message), "utf8"), 0x1);
  }

  sendFrame(payload, opcode) {
    const mask = crypto.randomBytes(4);
    let header;
    if (payload.length < 126) {
      header = Buffer.alloc(2);
      header[1] = 0x80 | payload.length;
    } else if (payload.length <= 0xffff) {
      header = Buffer.alloc(4);
      header[1] = 0x80 | 126;
      header.writeUInt16BE(payload.length, 2);
    } else {
      header = Buffer.alloc(10);
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(payload.length), 2);
    }
    header[0] = 0x80 | opcode;
    const masked = Buffer.alloc(payload.length);
    for (let index = 0; index < payload.length; index += 1) {
      masked[index] = payload[index] ^ mask[index % 4];
    }
    this.socket.write(Buffer.concat([header, mask, masked]));
  }

  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      if (this.buffer.length < 2) return;
      const first = this.buffer[0];
      const second = this.buffer[1];
      const opcode = first & 0x0f;
      const masked = (second & 0x80) !== 0;
      let length = second & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (this.buffer.length < offset + 2) return;
        length = this.buffer.readUInt16BE(offset);
        offset += 2;
      } else if (length === 127) {
        if (this.buffer.length < offset + 8) return;
        const bigLength = this.buffer.readBigUInt64BE(offset);
        if (bigLength > BigInt(Number.MAX_SAFE_INTEGER)) {
          this.socket.destroy(new Error("Chrome DevTools frame too large"));
          return;
        }
        length = Number(bigLength);
        offset += 8;
      }
      let mask = null;
      if (masked) {
        if (this.buffer.length < offset + 4) return;
        mask = this.buffer.slice(offset, offset + 4);
        offset += 4;
      }
      if (this.buffer.length < offset + length) return;
      let payload = this.buffer.slice(offset, offset + length);
      this.buffer = this.buffer.slice(offset + length);
      if (mask) {
        payload = Buffer.from(payload.map((byte, index) => byte ^ mask[index % 4]));
      }
      if (opcode === 0x8) {
        this.close();
        return;
      }
      if (opcode === 0x9) {
        this.sendFrame(payload, 0xA);
        continue;
      }
      if (opcode !== 0x1) {
        continue;
      }
      this.onMessage(payload.toString("utf8"));
    }
  }

  onMessage(text) {
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (!message.id || !this.pending.has(message.id)) {
      return;
    }
    const slot = this.pending.get(message.id);
    this.pending.delete(message.id);
    clearTimeout(slot.timer);
    if (message.error) {
      slot.reject(new Error(`CDP ${slot.method} failed: ${JSON.stringify(message.error)}`));
    } else {
      slot.resolve(message.result);
    }
  }

  rejectAll(error) {
    for (const slot of this.pending.values()) {
      clearTimeout(slot.timer);
      slot.reject(error);
    }
    this.pending.clear();
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    try {
      this.sendFrame(Buffer.alloc(0), 0x8);
    } catch {
      // ignore close races
    }
    this.socket.end();
  }
}

async function evaluateExpression(target, expression, timeoutMs = 5000) {
  if (!target || !target.webSocketDebuggerUrl) {
    throw new Error(`target does not include webSocketDebuggerUrl: ${JSON.stringify(target)}`);
  }
  const cdp = await CdpConnection.connect(target.webSocketDebuggerUrl, timeoutMs);
  try {
    await cdp.request("Runtime.enable", {}, timeoutMs);
    const evaluated = await cdp.request(
      "Runtime.evaluate",
      {
        expression,
        returnByValue: true,
        awaitPromise: true,
      },
      timeoutMs,
    );
    if (evaluated.exceptionDetails) {
      throw new Error(`Runtime.evaluate exception: ${JSON.stringify(evaluated.exceptionDetails)}`);
    }
    return evaluated.result?.value;
  } finally {
    cdp.close();
  }
}

module.exports = {
  CdpConnection,
  createTarget,
  evaluateExpression,
  listTargets,
  normalizeEndpoint,
  waitForDevToolsEndpoint,
};
