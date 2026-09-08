import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { canonicalFixtureBytes, createCamera, framebufferSize, makeFixture, sampleLayer, screenToWorld, worldToScreen } from './fixture.js';
import { TileCache, classifyFloorPixels } from './renderer.js';

test('shared fixture has explicit provenance and honest unknown values', () => {
  const fixture = makeFixture();
  assert.equal(fixture.provenance.seed, 0x6d2b79f5);
  assert.equal(fixture.layers.length, 120);
  assert.equal(fixture.overlays.count, 10_000);
  assert.equal(fixture.checks.unknownCellsAreNull, true);
  const unknown = fixture.layers[0].mask.findIndex((mask) => mask === 0);
  assert.notEqual(unknown, -1);
  assert.equal(fixture.layers[0].values[unknown], Number.NaN); // NaN is checked below without coercion.
});

test('world/screen mapping preserves scale and axis direction', () => {
  const fixture = makeFixture();
  const camera = createCamera(fixture);
  const viewport = { width: 1200, height: 720 };
  const source = [731.25, 1412.5];
  const screen = worldToScreen(source, camera, viewport);
  const roundTrip = screenToWorld([screen.x, screen.y], camera, viewport);
  assert.ok(Math.hypot(roundTrip[0] - source[0], roundTrip[1] - source[1]) < 1e-9);
  const top = worldToScreen([source[0], source[1] - 100], camera, viewport);
  assert.ok(top.y < screen.y, 'local y-down world must move upward on screen when its value decreases');
  assert.throws(() => sampleLayer(fixture.layers[0], 256, 0), /outside/);
});

test('DPR framebuffer sizing preserves CSS dimensions and rejects malformed inputs', () => {
  assert.deepEqual(framebufferSize(340, 479.078125, 2), [680, 958]);
  assert.deepEqual(framebufferSize(1020, 610, 1), [1020, 610]);
  assert.throws(() => framebufferSize(340, 479, 0), /invalid framebuffer/);
});

test('canonical fixture bytes produce a full SHA-256 content hash', () => {
  const fixture = makeFixture();
  const bytes = canonicalFixtureBytes(fixture);
  const digest = createHash('sha256').update(bytes).digest('hex');
  assert.ok(bytes.byteLength > 20_000_000, `canonical fixture unexpectedly small: ${bytes.byteLength}`);
  assert.equal(digest.length, 64);
  assert.equal(createHash('sha256').update(canonicalFixtureBytes(makeFixture())).digest('hex'), digest);
});

test('unknown mask probe returns null rather than a fabricated numeric value', () => {
  const fixture = makeFixture();
  const layer = fixture.layers[0];
  const index = layer.mask.findIndex((mask) => mask === 0);
  const x = index % layer.width;
  const y = Math.floor(index / layer.width);
  assert.equal(sampleLayer(layer, x, y).value, null);
  assert.equal(sampleLayer(layer, x, y).mask, 0);
});

test('tile cache remains bounded and cancellation is observable', async () => {
  const cache = new TileCache(4);
  await cache.stream(40);
  assert.ok(cache.entries.size <= 4, `cache grew to ${cache.entries.size}`);
  assert.ok(cache.stats.evictions > 0, 'stream should exercise eviction');
  assert.ok(cache.stats.cancelled > 0, 'stream should exercise cancellation');
});

test('viewport tile requests prioritize the center and cancel superseded navigation', async () => {
  const cache = new TileCache(4);
  const first = cache.beginViewport({ x: 2048, y: 1536, zoom: 1, worldWidth: 4096, worldHeight: 3072 });
  const second = cache.beginViewport({ x: 3000, y: 2100, zoom: 2, worldWidth: 4096, worldHeight: 3072 });
  const [firstResult, secondResult] = await Promise.all([first, second]);
  assert.equal(firstResult.priority[0], '3/4/4', 'first viewport must request its center tile first');
  assert.equal(secondResult.priority[0], '4/11/10', 'second viewport must request its center tile first');
  assert.ok(firstResult.cancelled > 0, 'superseded viewport must expose cancellation');
  assert.equal(secondResult.camera.x, 3000);
  assert.ok(firstResult.started.length > 0 && firstResult.started[0] === firstResult.priority[0], 'viewport scheduler must launch the center tile first');
  assert.ok(firstResult.peakActive <= firstResult.concurrencyCap, 'viewport scheduler must respect its concurrency cap');
  assert.ok(secondResult.peakActive <= secondResult.concurrencyCap, 'current viewport scheduler must respect its concurrency cap');
  assert.ok(cache.entries.size <= 4, `viewport cache grew to ${cache.entries.size}`);
});

test('3D floor framebuffer probe rejects a no-op draw mutation', () => {
  const colors = [[242, 46, 46], [46, 210, 76]];
  const valid = new Uint8Array([...colors[0], 255, ...colors[1], 255]);
  assert.deepEqual(classifyFloorPixels(valid, colors), [1, 1]);
  assert.deepEqual(classifyFloorPixels(new Uint8Array([8, 12, 20, 255]), colors), [0, 0], 'a no-op/background framebuffer must fail the floor-specific assertion');
});
