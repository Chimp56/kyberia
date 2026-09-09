#!/usr/bin/env node
/**
 * Rebuild the Rust scene-admission WASM with the repository toolchain and
 * verify that the checked-in browser artifact is byte-for-byte identical.
 * This command never writes the reviewed artifact; Cargo's ignored target
 * directory is the only build output.
 */
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join, relative, resolve } from 'node:path';
import { homedir } from 'node:os';

const RENDERER = dirname(fileURLToPath(import.meta.url));
const ROOT = join(RENDERER, '..', '..');
const TOOLCHAIN = /^channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$/m.exec(readFileSync(join(ROOT, 'rust-toolchain.toml'), 'utf8'))?.[1];
if (!TOOLCHAIN) throw new Error('WASM reproducibility requires an exact repository Rust toolchain pin');
const SUPPORT_MANIFEST = join(RENDERER, 'support', 'scene-validator-wasm', 'Cargo.toml');
const TARGET_DIR = join(RENDERER, 'support', 'scene-validator-wasm', 'target');
const BUILT_ARTIFACT = join(RENDERER, 'support', 'scene-validator-wasm', 'target', 'wasm32-unknown-unknown', 'release', 'kyberia_renderer_scene_validator_wasm.wasm');
const REVIEWED_ARTIFACT = join(RENDERER, 'wasm', 'kyberia_scene_validator_wasm.wasm');
const DIGEST_FILE = join(RENDERER, 'wasm', 'kyberia_scene_validator_wasm.wasm.sha256');
const ARTIFACT_NAME = 'kyberia_scene_validator_wasm.wasm';

export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function parseDigest(text) {
  const match = /^([0-9a-f]{64})  kyberia_scene_validator_wasm\.wasm$/.exec(text.trim());
  if (!match) throw new Error(`${ARTIFACT_NAME}.sha256 is not a canonical sha256sum record`);
  return match[1];
}

export function verifyDigests({ expectedDigest, reviewedBytes, rebuiltBytes }) {
  const reviewedDigest = sha256(reviewedBytes);
  const rebuiltDigest = sha256(rebuiltBytes);
  if (reviewedDigest !== expectedDigest) {
    throw new Error(`checked-in ${ARTIFACT_NAME} hash ${reviewedDigest} differs from sidecar ${expectedDigest}`);
  }
  if (rebuiltDigest !== expectedDigest) {
    throw new Error(`reproducible Rust build hash ${rebuiltDigest} differs from reviewed artifact ${expectedDigest}`);
  }
  if (reviewedBytes.byteLength !== rebuiltBytes.byteLength || !reviewedBytes.every((byte, index) => byte === rebuiltBytes[index])) {
    throw new Error('reproducible Rust build bytes differ from the reviewed artifact');
  }
  return { digest: expectedDigest, byteLength: reviewedBytes.byteLength };
}

export function buildArguments() {
  return [
    'build',
    '--manifest-path', relative(ROOT, SUPPORT_MANIFEST),
    '--target', 'wasm32-unknown-unknown',
    '--target-dir', relative(ROOT, TARGET_DIR),
    '--release',
    '--locked',
    '--offline',
  ];
}

export function buildEnvironment(environment = process.env, root = ROOT) {
  const result = { ...environment };
  delete result.CARGO_TARGET_DIR;
  delete result.CARGO_BUILD_TARGET_DIR;
  for (const key of Object.keys(result)) {
    if (key.startsWith('CARGO_PROFILE_') || key.startsWith('RUSTC')
      || key.startsWith('CARGO_BUILD_RUSTC')
      || key.startsWith('CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_')) delete result[key];
  }
  result.RUSTUP_TOOLCHAIN = TOOLCHAIN;
  // Panic locations include absolute dependency paths even in release builds.
  // Own the compiler flags so checkout and registry locations do not enter
  // the distributed artifact or change its digest across worktrees.
  delete result.RUSTFLAGS;
  delete result.CARGO_BUILD_RUSTFLAGS;
  delete result.CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS;
  const cargoHome = resolve(environment.CARGO_HOME || join(homedir(), '.cargo'));
  result.CARGO_ENCODED_RUSTFLAGS = [
    `--remap-path-prefix=${resolve(root)}=/kyberia`,
    `--remap-path-prefix=${cargoHome}=/cargo`,
  ].join('\u001f');
  return result;
}

function runCargoBuild() {
  return new Promise((resolve, reject) => {
    const child = spawn('cargo', buildArguments(), { cwd: ROOT, env: buildEnvironment(), stdio: 'inherit' });
    child.once('error', (error) => reject(new Error(`reproducible WASM build could not start: ${error.message}`)));
    child.once('exit', (code, signal) => {
      if (code !== 0) reject(new Error(`reproducible WASM build failed${signal ? ` (${signal})` : ` with exit code ${code}`}; target/toolchain/dependencies may be unavailable`));
      else resolve();
    });
  });
}

export async function checkReproducibleArtifact() {
  await runCargoBuild();
  let reviewedBytes;
  let rebuiltBytes;
  let expectedDigest;
  try {
    [reviewedBytes, rebuiltBytes] = await Promise.all([readFile(REVIEWED_ARTIFACT), readFile(BUILT_ARTIFACT)]);
    expectedDigest = parseDigest(await readFile(DIGEST_FILE, 'utf8'));
  } catch (error) {
    throw new Error(`reproducible WASM build completed without a readable ${ARTIFACT_NAME}: ${error.message}`);
  }
  return verifyDigests({ expectedDigest, reviewedBytes, rebuiltBytes });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const result = await checkReproducibleArtifact();
    console.log(`PASS: ${ARTIFACT_NAME} reproduces byte-for-byte (${result.byteLength} bytes, sha256=${result.digest})`);
  } catch (error) {
    console.error(`FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
