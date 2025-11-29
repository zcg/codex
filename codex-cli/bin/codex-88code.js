#!/usr/bin/env node
// 88code CLI entry point - loads platform-specific binary from optional dependencies.

import { spawn } from "node:child_process";
import { existsSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { createRequire } from "module";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

const { platform, arch } = process;

// Map Node.js platform/arch to package names
const PLATFORM_PACKAGES = {
  "darwin-arm64": "@88code/codex-darwin-arm64",
  "darwin-x64": "@88code/codex-darwin-x64",
  "linux-arm64": "@88code/codex-linux-arm64",
  "linux-x64": "@88code/codex-linux-x64",
  "win32-arm64": "@88code/codex-win32-arm64",
  "win32-x64": "@88code/codex-win32-x64",
};

const platformKey = `${platform}-${arch}`;
const packageName = PLATFORM_PACKAGES[platformKey];

if (!packageName) {
  console.error(`Unsupported platform: ${platform} (${arch})`);
  process.exit(1);
}

// Find the platform-specific package
let binaryPath;
try {
  const packageDir = path.dirname(require.resolve(`${packageName}/package.json`));
  const binaryName = platform === "win32" ? "codex.exe" : "codex";
  binaryPath = path.join(packageDir, binaryName);
} catch (e) {
  console.error(`Platform package ${packageName} not found.`);
  console.error(`Please reinstall: npm install -g @88code/codex`);
  process.exit(1);
}

if (!existsSync(binaryPath)) {
  console.error(`Binary not found at ${binaryPath}`);
  process.exit(1);
}

function getUpdatedPath(newDirs) {
  const pathSep = platform === "win32" ? ";" : ":";
  const existingPath = process.env.PATH || "";
  return [...newDirs, ...existingPath.split(pathSep).filter(Boolean)].join(pathSep);
}

function detectPackageManager() {
  const userAgent = process.env.npm_config_user_agent || "";
  if (/\bbun\//.test(userAgent)) return "bun";
  const execPath = process.env.npm_execpath || "";
  if (execPath.includes("bun")) return "bun";
  if (process.env.BUN_INSTALL || process.env.BUN_INSTALL_GLOBAL_DIR || process.env.BUN_INSTALL_BIN_DIR) {
    return "bun";
  }
  return userAgent ? "npm" : null;
}

const env = { ...process.env };
const packageManagerEnvVar = detectPackageManager() === "bun" ? "CODEX_MANAGED_BY_BUN" : "CODEX_MANAGED_BY_NPM";
env[packageManagerEnvVar] = "1";

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  console.error(err);
  process.exit(1);
});

const forwardSignal = (signal) => {
  if (child.killed) return;
  try {
    child.kill(signal);
  } catch {}
};

["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) => {
  process.on(sig, () => forwardSignal(sig));
});

const childResult = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    if (signal) {
      resolve({ type: "signal", signal });
    } else {
      resolve({ type: "code", exitCode: code ?? 1 });
    }
  });
});

if (childResult.type === "signal") {
  process.kill(process.pid, childResult.signal);
} else {
  process.exit(childResult.exitCode);
}
