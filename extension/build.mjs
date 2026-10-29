#!/usr/bin/env node
/**
 * Validate and assemble the Chromium and Firefox extension builds.
 *
 * Usage:
 *   node extension/build.mjs          # validate + write dist/chrome, dist/firefox
 *   node extension/build.mjs --check  # validate only (used by CI)
 */

import { access, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
const checkOnly = process.argv.includes("--check");

const targets = [
  { name: "chrome", manifest: "manifest.chrome.json", background: "service_worker" },
  { name: "firefox", manifest: "manifest.firefox.json", background: "scripts" },
];

async function exists(relativePath) {
  try {
    await access(path.join(root, relativePath));
    return true;
  } catch {
    return false;
  }
}

async function validate(target, manifest) {
  const errors = [];

  if (manifest.manifest_version !== 3) {
    errors.push("manifest_version must be 3");
  }
  if (!manifest.name || !manifest.version) {
    errors.push("name and version are required");
  }
  if (!manifest.icons || !manifest.icons["128"]) {
    errors.push("a 128px icon is required");
  }

  const backgroundPaths =
    target.background === "service_worker"
      ? [manifest.background?.service_worker]
      : Array.isArray(manifest.background?.scripts)
        ? manifest.background.scripts
        : [];
  if (backgroundPaths.length === 0 || backgroundPaths.some((value) => !value)) {
    errors.push(`background.${target.background} is missing`);
  }

  const referenced = new Set([
    manifest.action?.default_popup,
    ...backgroundPaths,
    ...(manifest.content_scripts ?? []).flatMap((script) => script.js ?? []),
    ...Object.values(manifest.icons ?? {}),
    ...Object.values(manifest.action?.default_icon ?? {}),
  ]);

  for (const value of referenced) {
    if (!value) continue;
    if (!(await exists(value))) {
      errors.push(`missing file: ${value}`);
    }
  }

  if (!manifest.permissions?.includes("nativeMessaging")) {
    errors.push("nativeMessaging permission is required");
  }

  if (errors.length > 0) {
    throw new Error(`${target.name} manifest:\n  - ${errors.join("\n  - ")}`);
  }

  return referenced.size;
}

let validatedFiles = 0;
for (const target of targets) {
  const manifest = JSON.parse(await readFile(path.join(root, target.manifest), "utf8"));
  validatedFiles += await validate(target, manifest);

  if (!checkOnly) {
    const output = path.join(root, "dist", target.name);
    await rm(output, { recursive: true, force: true });
    await mkdir(output, { recursive: true });
    await cp(path.join(root, "src"), path.join(output, "src"), { recursive: true });
    await cp(path.join(root, "icons"), path.join(output, "icons"), { recursive: true });
    await writeFile(
      path.join(output, "manifest.json"),
      `${JSON.stringify(manifest, null, 2)}\n`,
    );
    console.log(`built extension/dist/${target.name}`);
  }
}

console.log(
  `${checkOnly ? "validated" : "built"} ${targets.length} extension targets (${validatedFiles} referenced files checked)`,
);
