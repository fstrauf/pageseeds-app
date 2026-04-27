#!/usr/bin/env node
/**
 * IPC Surface Check
 *
 * Compares frontend `invoke('command')` calls against Tauri command registrations
 * in `src-tauri/src/lib.rs`. Fails if any statically-invoked command is not registered.
 *
 * Usage:
 *   node scripts/check-ipc-surface.mjs
 *
 * Allowlist:
 *   Create `scripts/ipc-allowlist.json` with backend-only commands:
 *   { "registeredButNotInvoked": ["execute_task", "dry_run_task"] }
 */

import { readFileSync, existsSync, readdirSync, statSync } from "fs";
import { dirname, join, extname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");

// ─── Helpers ─────────────────────────────────────────────────────────────────

function* walkDir(dir) {
  const entries = readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      yield* walkDir(fullPath);
    } else {
      yield fullPath;
    }
  }
}

// ─── Parse registered commands from lib.rs ───────────────────────────────────

const libRsPath = join(root, "src-tauri", "src", "lib.rs");
const libRs = readFileSync(libRsPath, "utf-8");

// Extract commands::name from generate_handler![...]
const handlerMatch = libRs.match(/tauri::generate_handler!\s*\[([^\]]*)\]/s);
if (!handlerMatch) {
  console.error("[check-ipc] FAIL: Could not find generate_handler! block in lib.rs");
  process.exit(1);
}

const registered = new Set(
  [...handlerMatch[1].matchAll(/commands::([a-zA-Z0-9_]+)/g)].map((m) => m[1])
);

// ─── Parse invoked commands from frontend ────────────────────────────────────

const srcDir = join(root, "src");
const invoked = new Set();
const invokeLocations = new Map(); // command -> [{file, line}]

for (const absPath of walkDir(srcDir)) {
  const ext = extname(absPath);
  if (ext !== ".ts" && ext !== ".tsx") continue;

  const relPath = absPath.slice(root.length + 1);
  const content = readFileSync(absPath, "utf-8");
  const lines = content.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    // Match invoke('command') or invoke<Type>('command')
    const matches = line.matchAll(/invoke(?:<[^>]+>)?\('([^']+)'/g);
    for (const match of matches) {
      const cmd = match[1];
      invoked.add(cmd);
      if (!invokeLocations.has(cmd)) {
        invokeLocations.set(cmd, []);
      }
      invokeLocations.get(cmd).push({ file: relPath, line: i + 1 });
    }
  }
}

// ─── Load allowlist ──────────────────────────────────────────────────────────

const allowlistPath = join(root, "scripts", "ipc-allowlist.json");
let allowlist = { registeredButNotInvoked: [] };
if (existsSync(allowlistPath)) {
  try {
    allowlist = JSON.parse(readFileSync(allowlistPath, "utf-8"));
  } catch (e) {
    console.error(`[check-ipc] WARN: Failed to parse ${allowlistPath}: ${e.message}`);
  }
}
const allowedBackendOnly = new Set(allowlist.registeredButNotInvoked || []);

// ─── Compare and report ──────────────────────────────────────────────────────

const invokedButNotRegistered = [...invoked].filter((c) => !registered.has(c));
const registeredButNotInvoked = [...registered].filter(
  (c) => !invoked.has(c) && !allowedBackendOnly.has(c)
);

console.log(`[check-ipc] Registered commands: ${registered.size}`);
console.log(`[check-ipc] Frontend invoke calls: ${invoked.size}`);

let failed = false;

if (invokedButNotRegistered.length > 0) {
  console.error(`\n[check-ipc] ERROR: ${invokedButNotRegistered.length} command(s) invoked but not registered:`);
  for (const cmd of invokedButNotRegistered.sort()) {
    const locs = invokeLocations.get(cmd) || [];
    for (const loc of locs.slice(0, 3)) {
      console.error(`  - ${cmd}  (${loc.file}:${loc.line})`);
    }
    if (locs.length > 3) {
      console.error(`    ... and ${locs.length - 3} more location(s)`);
    }
  }
  failed = true;
}

if (registeredButNotInvoked.length > 0) {
  console.warn(`\n[check-ipc] WARN: ${registeredButNotInvoked.length} registered command(s) with no frontend invoke (not allowlisted):`);
  for (const cmd of registeredButNotInvoked.sort()) {
    console.warn(`  - ${cmd}`);
  }
  console.warn(`\n  If these are intentionally backend-only, add them to scripts/ipc-allowlist.json`);
}

if (allowedBackendOnly.size > 0) {
  const actuallyAllowed = [...allowedBackendOnly].filter((c) => registered.has(c) && !invoked.has(c));
  if (actuallyAllowed.length > 0) {
    console.log(`\n[check-ipc] INFO: ${actuallyAllowed.length} command(s) allowlisted as backend-only:`);
    for (const cmd of actuallyAllowed.sort()) {
      console.log(`  - ${cmd}`);
    }
  }
}

if (failed) {
  console.error("\n[check-ipc] FAIL");
  process.exit(1);
}

console.log("\n[check-ipc] OK: All frontend invokes match registered commands.");
