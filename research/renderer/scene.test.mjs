import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { loadCanonicalScene, loadCanonicalSceneMirror, MAX_SCENE_BYTES, MAX_WASM_ADMISSION_MS, SceneLoadError, sceneCellCenter, validateCanonicalSceneWithWasm } from './scene.js';

const encoder = new TextEncoder();
const fixturePath = new URL('./fixtures/canonical-scene-v1.json', import.meta.url);
const fixtureBytes = await readFile(fixturePath);
const wasmPath = new URL('./wasm/kyberia_scene_validator_wasm.wasm', import.meta.url);
const wasmBytes = await readFile(wasmPath);
const wasmDigestPath = new URL('./wasm/kyberia_scene_validator_wasm.wasm.sha256', import.meta.url);
const wasmDigest = (await readFile(wasmDigestPath, 'utf8')).trim().split(/\s+/, 1)[0];
const pointFixtureBytes = await readFile(new URL('./fixtures/canonical-scene-point-v1.json', import.meta.url));
const nearestFixtureBytes = await readFile(new URL('./fixtures/canonical-scene-nearest-v1.json', import.meta.url));

async function loadFixture() { return loadCanonicalSceneMirror(fixtureBytes); }
function mutateJson(mutator) {
  const value = JSON.parse(new TextDecoder().decode(fixtureBytes));
  mutator(value);
  return encoder.encode(JSON.stringify(value));
}
async function rejects(bytes, state, code) {
  await assert.rejects(loadCanonicalSceneMirror(bytes), (error) => error instanceof SceneLoadError && error.state === state && error.code === code);
}

async function rustAdmission(bytes) {
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const input = new Uint8Array(bytes);
  const pointer = instance.exports.input_ptr(input.byteLength);
  new Uint8Array(instance.exports.memory.buffer, pointer, input.byteLength).set(input);
  return instance.exports.validate_scene(input.byteLength);
}

test('canonical scene preserves contract, provenance, numeric values, masks, and cell geometry', async () => {
  const loaded = await loadFixture();
  assert.equal(loaded.contract, 'kyberia.render-scene/1');
  assert.equal(loaded.wireSchema, 'V1');
  assert.equal(loaded.evidencePlane, 'Synthetic');
  assert.equal(loaded.byteLength, fixtureBytes.byteLength);
  assert.equal(loaded.sha256, '24c2765339aceef830d05e57999e3b435a10092d2782ed9c2153e2c1e306fc8c');
  assert.equal(loaded.counts.cells, 48);
  assert.equal(loaded.counts.knownCells, 17);
  assert.equal(loaded.counts.unknownCells, 31);
  assert.equal(loaded.layer.unit, 'dBm');
  assert.deepEqual(loaded.layer.worldBounds, [0, 0, 8, 6]);
  assert.ok(Number.isNaN(loaded.layer.values[0]));
  assert.equal(loaded.layer.mask[0], 0);
  assert.equal(loaded.layer.mask[9], 1);
  assert.deepEqual(sceneCellCenter(loaded.scene, 9), { x: 1.5, y: 1.5, column: 1, row: 1 });
  assert.equal(loaded.scene.cells[9].value.detail, -42);
});

test('checked-in WASM admission artifact is bound to its reviewed digest', () => {
  assert.equal(createHash('sha256').update(wasmBytes).digest('hex'), wasmDigest);
});

test('numerical replay rejects changed values and classes', async () => {
  await rejects(mutateJson((scene) => { scene.cells[9].value.detail = -41; }), 'invalid', 'numerical-replay');
  await rejects(mutateJson((scene) => { scene.cells[9].class = 'interpolated'; }), 'invalid', 'numerical-replay');
});

test('strict schema rejects unknown fields, unsupported versions, nonfinite values, and bad metric hashes', async () => {
  await rejects(mutateJson((scene) => { scene.unexpected = true; }), 'invalid', 'schema-fields');
  await rejects(mutateJson((scene) => { scene.schema = 'V2'; }), 'unsupported', 'schema-version');
  await rejects(encoder.encode(new TextDecoder().decode(fixtureBytes).replace('"resolution":1.0', '"resolution":1e999')), 'invalid', 'number');
  await rejects(mutateJson((scene) => { scene.metric_definition_bytes[0] ^= 1; }), 'invalid', 'hash');
});

test('canonical admission rejects pretty JSON, duplicate keys, and reordered object fields before drawing', async () => {
  const source = new TextDecoder().decode(fixtureBytes);
  await rejects(encoder.encode(JSON.stringify(JSON.parse(source), null, 2)), 'invalid', 'canonical-json');
  await rejects(encoder.encode(source.replace('{"schema":"V1"', '{"schema":"V1","schema":"V1"')), 'invalid', 'canonical-json');
  const value = JSON.parse(source);
  const reordered = { identity: value.identity, schema: value.schema, metric_artifact: value.metric_artifact, metric_definition_bytes: value.metric_definition_bytes, evidence_plane: value.evidence_plane, configuration: value.configuration, samples: value.samples, grid: value.grid, location_groups: value.location_groups, cells: value.cells };
  await rejects(encoder.encode(JSON.stringify(reordered)), 'invalid', 'schema-fields');
});

test('Rust canonical admission rejects alternate string and number spellings', async () => {
  const source = new TextDecoder().decode(fixtureBytes);
  const escapedSchema = encoder.encode(source.replace('"schema":"V1"', '"schema":"\\u00561"'));
  const floatWidth = encoder.encode(source.replace('"width":8', '"width":8.0'));
  assert.equal(await rustAdmission(fixtureBytes), 0);
  assert.equal(await rustAdmission(escapedSchema), 6, 'escaped schema spelling must be rejected by the Rust canonical contract');
  assert.notEqual(await rustAdmission(floatWidth), 0, 'alternate numeric spelling must be rejected by the Rust canonical contract');
});

test('Rust and browser replay admit unit enum method scenes for point and nearest semantics', async () => {
  for (const [method, bytes] of [['PointValue', pointFixtureBytes], ['Nearest', nearestFixtureBytes]]) {
    assert.equal(await rustAdmission(bytes), 0, `${method} fixture must pass Rust canonical admission`);
    const loaded = await loadCanonicalSceneMirror(bytes);
    assert.equal(loaded.scene.configuration.method, method);
    assert.equal(loaded.scene.cells[9].class, 'observed', `${method} exact evidence must remain observed`);
    assert.equal(loaded.scene.cells[9].value.detail, -42);
    if (method === 'Nearest') {
      assert.equal(loaded.scene.cells[11].class, 'interpolated');
      assert.equal(loaded.scene.cells[11].value.detail, -42);
    } else {
      assert.equal(loaded.scene.cells[10].class, 'unknown');
    }
  }
});

test('canonical admission binds metric artifact version/media type and replays aggregate and uncertainty semantics', async () => {
  await rejects(mutateJson((scene) => { scene.metric_artifact.version = 'wifi.rssi.idw/2'; }), 'invalid', 'identity');
  await rejects(mutateJson((scene) => { scene.metric_artifact.media_type = 'application/json'; }), 'invalid', 'identity');
  await rejects(mutateJson((scene) => { scene.location_groups[0].signal_aggregate.estimate.detail = -41; }), 'invalid', 'numerical-replay');
  await rejects(mutateJson((scene) => { scene.cells[9].uncertainty_db = { state: 'known', detail: 99 }; }), 'invalid', 'numerical-replay');
});

test('canonical layer geometry follows a translated grid instead of synthetic fixture dimensions', async () => {
  const shifted = mutateJson((scene) => {
    const dx = 100;
    const dy = 200;
    scene.grid.origin = { x: dx, y: dy };
    for (const sample of scene.samples) { sample.position.x += dx; sample.position.y += dy; }
    for (const group of scene.location_groups) { group.position.x += dx; group.position.y += dy; }
  });
  const loaded = await loadCanonicalSceneMirror(shifted);
  assert.deepEqual(loaded.layer.worldBounds, [100, 200, 108, 206]);
  assert.deepEqual(sceneCellCenter(loaded.scene, 9), { x: 101.5, y: 201.5, column: 1, row: 1 });
});

test('preflight rejects an oversized array before JSON parsing can retain its contents', async () => {
  const values = Array.from({ length: 100_001 }, () => '0').join(',');
  await rejects(encoder.encode(`{"schema":"V1","samples":[${values},BROKEN]}`), 'resource-limit', 'array-items');
});

test('metric admission rejects a same-unit impostor even when its replacement hash is self-consistent', async () => {
  await rejects(mutateJson((scene) => {
    const definition = JSON.parse(new TextDecoder().decode(Uint8Array.from(scene.metric_definition_bytes)));
    definition.semantic_description = 'arbitrary dBm values';
    const replacement = encoder.encode(JSON.stringify(definition));
    const digest = createHash('sha256').update(replacement).digest('hex');
    scene.metric_definition_bytes = Array.from(replacement);
    scene.metric_artifact.byte_length = String(replacement.length);
    scene.metric_artifact.sha256 = digest;
    scene.identity.metric_definition_hash = digest;
  }), 'invalid', 'metric-definition');
});

test('bounded preflight rejects oversized input before parsing', async () => {
  await rejects(new Uint8Array(MAX_SCENE_BYTES + 1), 'resource-limit', 'scene-bytes');
});

test('cancellation is observable before and during bounded scene admission', async () => {
  await assert.rejects(loadCanonicalSceneMirror(fixtureBytes, { isCancelled: () => true }), (error) => error.state === 'cancelled');
  let polls = 0;
  await assert.rejects(loadCanonicalSceneMirror(fixtureBytes, { isCancelled: () => { polls += 1; return polls > 8; } }), (error) => error.state === 'cancelled');
  assert.ok(polls > 1, 'cancellation must be polled during the admission pipeline');
});

test('canonical loader cannot omit the Rust admission boundary', async () => {
  await assert.rejects(loadCanonicalScene(fixtureBytes), (error) => error instanceof SceneLoadError && error.state === 'unsupported' && error.code === 'wasm-runtime');
});

test('canonical loader invokes Rust admission before publishing a valid scene', async () => {
  const previousWorker = globalThis.Worker;
  let posted = false;
  globalThis.Worker = class AcceptingWorker {
    postMessage(message) {
      posted = message.id === 1 && message.bytes instanceof ArrayBuffer;
      queueMicrotask(() => this.onmessage?.({ data: { id: 1, status: 0 } }));
    }
    terminate() {}
  };
  try {
    const loaded = await loadCanonicalScene(fixtureBytes);
    assert.equal(posted, true);
    assert.equal(loaded.contract, 'kyberia.render-scene/1');
  } finally {
    globalThis.Worker = previousWorker;
  }
});

test('WASM admission has a finite deadline and terminates a silent worker', async () => {
  const previousWorker = globalThis.Worker;
  let terminated = false;
  globalThis.Worker = class SilentWorker {
    postMessage() {}
    terminate() { terminated = true; }
  };
  try {
    await assert.rejects(validateCanonicalSceneWithWasm(fixtureBytes, { deadlineMs: 10 }), (error) => error instanceof SceneLoadError && error.state === 'resource-limit' && error.code === 'wasm-timeout');
    assert.equal(terminated, true);
  } finally {
    globalThis.Worker = previousWorker;
  }
});

test('WASM admission cancellation terminates a pending worker', async () => {
  const previousWorker = globalThis.Worker;
  let terminated = false;
  let polls = 0;
  globalThis.Worker = class PendingWorker {
    postMessage() {}
    terminate() { terminated = true; }
  };
  try {
    await assert.rejects(validateCanonicalSceneWithWasm(fixtureBytes, { deadlineMs: MAX_WASM_ADMISSION_MS, isCancelled: () => { polls += 1; return polls > 1; } }), (error) => error instanceof SceneLoadError && error.state === 'cancelled');
    assert.equal(terminated, true);
  } finally {
    globalThis.Worker = previousWorker;
  }
});
