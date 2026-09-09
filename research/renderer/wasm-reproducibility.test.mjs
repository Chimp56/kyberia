import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { buildArguments, buildEnvironment, parseDigest, sha256, verifyDigests } from './check-wasm-reproducibility.mjs';

const artifact = await readFile(new URL('./wasm/kyberia_scene_validator_wasm.wasm', import.meta.url));
const sidecar = await readFile(new URL('./wasm/kyberia_scene_validator_wasm.wasm.sha256', import.meta.url), 'utf8');

test('reproducibility sidecar has the canonical sha256sum form', () => {
  assert.equal(parseDigest(sidecar), sha256(artifact));
});

test('reproducibility check rejects an altered rebuilt artifact', () => {
  const altered = Uint8Array.from(artifact);
  altered[0] ^= 1;
  assert.throws(() => verifyDigests({ expectedDigest: sha256(artifact), reviewedBytes: artifact, rebuiltBytes: altered }), /reproducible Rust build hash/);
});

test('reproducibility check rejects a malformed sidecar record', () => {
  assert.throws(() => parseDigest(`${sha256(artifact)} artifact.wasm`), /canonical sha256sum/);
});

test('reproducibility build pins its target directory and strips rerouting environment overrides', () => {
  const args = buildArguments();
  assert.deepEqual(args.slice(0, 7), [
    'build',
    '--manifest-path', join('research', 'renderer', 'support', 'scene-validator-wasm', 'Cargo.toml'),
    '--target', 'wasm32-unknown-unknown',
    '--target-dir', join('research', 'renderer', 'support', 'scene-validator-wasm', 'target'),
  ]);
  assert.equal(args.at(-3), '--release');
  assert.equal(args.at(-2), '--locked');
  assert.equal(args.at(-1), '--offline');
  const environment = buildEnvironment({ CARGO_TARGET_DIR: '/tmp/untrusted-target', CARGO_BUILD_TARGET_DIR: '/tmp/other-target', KEEP: 'present' });
  assert.equal(environment.CARGO_TARGET_DIR, undefined);
  assert.equal(environment.CARGO_BUILD_TARGET_DIR, undefined);
  assert.equal(environment.KEEP, 'present');
});

test('reproducibility build normalizes checkout and registry paths with owned compiler flags', () => {
  const environment = buildEnvironment({ CARGO_HOME: '/private/tmp/registry', RUSTFLAGS: 'untrusted', CARGO_ENCODED_RUSTFLAGS: 'untrusted', CARGO_BUILD_RUSTFLAGS: 'untrusted', CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS: 'untrusted' }, '/private/tmp/checkout');
  assert.equal(environment.RUSTFLAGS, undefined);
  assert.equal(environment.CARGO_BUILD_RUSTFLAGS, undefined);
  assert.equal(environment.CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS, undefined);
  assert.deepEqual(environment.CARGO_ENCODED_RUSTFLAGS.split('\u001f'), [
    `--remap-path-prefix=${resolve('/private/tmp/checkout')}=/kyberia`,
    `--remap-path-prefix=${resolve('/private/tmp/registry')}=/cargo`,
  ]);
});

test('reproducibility build rejects inherited compiler and profile selection through owned environment', () => {
  const environment = buildEnvironment({ RUSTUP_TOOLCHAIN: 'nightly', RUSTC: '/untrusted/compiler', RUSTC_WRAPPER: '/untrusted/wrapper', RUSTC_WORKSPACE_WRAPPER: '/untrusted/workspace-wrapper', CARGO_BUILD_RUSTC: '/untrusted/compiler2', CARGO_PROFILE_RELEASE_OPT_LEVEL: '0', CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER: '/untrusted/linker', CARGO_BUILD_JOBS: '2' });
  for (const key of ['RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_LINKER']) assert.equal(environment[key], undefined);
  assert.match(environment.RUSTUP_TOOLCHAIN, /^[0-9]+\.[0-9]+\.[0-9]+$/);
  assert.equal(environment.CARGO_BUILD_JOBS, '2');
});
