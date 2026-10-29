#!/usr/bin/env node
/**
 * Stage the native-messaging host for Tauri's externalBin bundler.
 *
 * Tauri requires sidecars to be named `<binary>-<target-triple>[.exe]` inside
 * `src-tauri/binaries`. The host is built by the Cargo workspace alongside the
 * app; this script copies it to the name the bundler expects.
 *
 * Usage: node scripts/prepare-sidecar.mjs [debug|release] [target-triple]
 */

import { copyFile, mkdir, stat } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const profile = process.argv[2] === "release" ? "release" : "debug";

function defaultTriple() {
  const { platform, arch } = process;
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "win32" && arch === "x64") return "x86_64-pc-windows-msvc";
  if (platform === "win32" && arch === "arm64") return "aarch64-pc-windows-msvc";
  throw new Error(`Unsupported host platform ${platform}/${arch}`);
}

const triple = process.argv[3] || process.env.RILLDOWN_TARGET_TRIPLE || defaultTriple();
const suffix = process.platform === "win32" ? ".exe" : "";
const binary = `rilldown-native-host${suffix}`;

const candidates = [
  path.join(root, "target", triple, profile, binary),
  path.join(root, "target", profile, binary),
];

let source = null;
for (const candidate of candidates) {
  try {
    const info = await stat(candidate);
    if (info.isFile()) {
      source = candidate;
      break;
    }
  } catch {
    // Try the next layout.
  }
}
if (!source) {
  throw new Error(
    `Native host is not built. Expected one of:\n${candidates.join("\n")}`,
  );
}

const outputDirectory = path.join(root, "src-tauri", "binaries");
const output = path.join(
  outputDirectory,
  `rilldown-native-host-${triple}${suffix}`,
);
await mkdir(outputDirectory, { recursive: true });
await copyFile(source, output);
console.log(`staged ${output}`);
