/**
 * RF Atlas renderer proof fixture.
 *
 * All arrays are generated from this file's seed.  There are no network
 * tiles, captures, or vendor assets in the benchmark.  The same Fixture
 * object is passed to both renderer candidates.
 */

export const FIXTURE_PROVENANCE = Object.freeze({
  fixtureId: 'renderer-gate-b-v1',
  seed: 0x6d2b79f5,
  generator: 'rfatlas-renderer-synthetic-v1',
  coordinateFrame: 'local-floorplan-millimetres',
  world: { width: 4096, height: 3072, unit: 'mm', origin: [0, 0], yDirection: 'down' },
  generatedAt: 'deterministic-seed-only',
  externalAssets: false,
});

export const WORLD_MILLIMETRES_PER_METRE = 1000;

class Rng {
  constructor(seed) { this.state = seed >>> 0; }
  next() {
    let x = this.state;
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    this.state = x >>> 0;
    return (this.state >>> 0) / 0x100000000;
  }
  between(min, max) { return min + (max - min) * this.next(); }
  int(min, max) { return Math.floor(this.between(min, max + 1)); }
}

function hash32(value) {
  let x = value >>> 0;
  x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
  x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
  return (x ^ (x >>> 16)) >>> 0;
}

function makeFloorplan(rng) {
  const walls = [];
  const rooms = [];
  const cols = 12;
  const rows = 9;
  const roomW = FIXTURE_PROVENANCE.world.width / cols;
  const roomH = FIXTURE_PROVENANCE.world.height / rows;
  for (let y = 0; y < rows; y += 1) {
    for (let x = 0; x < cols; x += 1) {
      const left = x * roomW;
      const top = y * roomH;
      const right = left + roomW;
      const bottom = top + roomH;
      rooms.push({ id: `room-${y}-${x}`, bounds: [left, top, right, bottom], floor: y % 6 });
      if (x < cols - 1 && (x + y) % 5 !== 2) {
        const gap = roomH * (0.18 + rng.next() * 0.12);
        walls.push({ x1: right, y1: top, x2: right, y2: top + gap });
        walls.push({ x1: right, y1: top + gap * 1.7, x2: right, y2: bottom });
      }
      if (y < rows - 1 && (x * 3 + y) % 7 !== 4) {
        const gap = roomW * (0.16 + rng.next() * 0.14);
        walls.push({ x1: left, y1: bottom, x2: left + gap, y2: bottom });
        walls.push({ x1: left + gap * 1.7, y1: bottom, x2: right, y2: bottom });
      }
    }
  }
  return { walls, rooms };
}

function makeNumericLayers(rng) {
  const width = 256;
  const height = 192;
  const count = 120;
  const layers = [];
  const cells = width * height;
  for (let layerIndex = 0; layerIndex < count; layerIndex += 1) {
    const values = new Float32Array(cells);
    const mask = new Uint8Array(cells);
    for (let i = 0; i < cells; i += 1) {
      const x = i % width;
      const y = Math.floor(i / width);
      const wave = Math.sin((x + layerIndex * 2.3) / 18) * 5;
      const radial = Math.cos((y - layerIndex * 1.7) / 21) * 4;
      values[i] = -44 + wave + radial - (layerIndex % 5) * 0.25 + rng.next() * 0.05;
      // 0 = unknown, 1 = observed, 2 = interpolated. Unknown cells carry no value.
      const unknown = ((x * 17 + y * 31 + layerIndex * 13) % 47) === 0 ||
        (x < 8 && y > height - 14) || (layerIndex % 11 === 0 && x > width - 12);
      mask[i] = unknown ? 0 : ((x + y + layerIndex) % 19 === 0 ? 2 : 1);
      if (unknown) values[i] = Number.NaN;
    }
    layers.push({ id: `metric-${String(layerIndex + 1).padStart(3, '0')}`, width, height, values, mask });
  }
  return layers;
}

function makeOverlays(rng) {
  const count = 10_000;
  const aps = new Float32Array(count * 4);
  const paths = new Float32Array(count * 4);
  for (let i = 0; i < count; i += 1) {
    const x = rng.between(16, FIXTURE_PROVENANCE.world.width - 16);
    const y = rng.between(16, FIXTURE_PROVENANCE.world.height - 16);
    aps[i * 4] = x;
    aps[i * 4 + 1] = y;
    aps[i * 4 + 2] = -30 - rng.between(0, 45);
    aps[i * 4 + 3] = i % 3;
    const pathX = (i * 37) % FIXTURE_PROVENANCE.world.width;
    const pathY = (i * 53) % FIXTURE_PROVENANCE.world.height;
    paths[i * 4] = pathX;
    paths[i * 4 + 1] = pathY;
    paths[i * 4 + 2] = (pathX + 80 + (i % 17) * 3) % FIXTURE_PROVENANCE.world.width;
    paths[i * 4 + 3] = (pathY + 52 + (i % 11) * 3) % FIXTURE_PROVENANCE.world.height;
  }
  return { count, aps, paths };
}

function makeFloors() {
  const floors = [];
  for (let floor = 0; floor < 6; floor += 1) {
    floors.push({ id: `floor-${floor + 1}`, z: floor * 3.4, width: 34, depth: 25, height: 0.22 });
  }
  return floors;
}

export function makeFixture() {
  const rng = new Rng(FIXTURE_PROVENANCE.seed);
  const fixture = {
    provenance: FIXTURE_PROVENANCE,
    floorplan: makeFloorplan(rng),
    layers: makeNumericLayers(rng),
    overlays: makeOverlays(rng),
    floors: makeFloors(),
    tile: { tileSize: 256, minZoom: 0, maxZoom: 5, cacheCapacity: 48 },
  };
  const firstUnknown = fixture.layers[0].mask.findIndex((mask, index) => mask === 0 && index % fixture.layers[0].width > 16 && Math.floor(index / fixture.layers[0].width) > 16 && index % fixture.layers[0].width < fixture.layers[0].width - 16 && Math.floor(index / fixture.layers[0].width) < fixture.layers[0].height - 16);
  const firstKnown = fixture.layers[0].mask.findIndex((mask) => mask !== 0);
  const cellPoint = (index) => ({ x: index % fixture.layers[0].width, y: Math.floor(index / fixture.layers[0].width) });
  fixture.probes = {
    knownCell: cellPoint(firstKnown),
    unknownCell: cellPoint(firstUnknown),
    wallPoint: fixture.floorplan.walls[Math.floor(fixture.floorplan.walls.length / 2)],
    overlayPoint: { x: fixture.overlays.aps[5000 * 4], y: fixture.overlays.aps[5000 * 4 + 1] },
  };
  fixture.checks = verifyFixture(fixture);
  return fixture;
}

export function worldToScreen(point, camera, viewport) {
  const scale = camera.zoom * Math.min(viewport.width / camera.worldWidth, viewport.height / camera.worldHeight);
  return {
    x: (point[0] - camera.x) * scale + viewport.width / 2,
    y: (point[1] - camera.y) * scale + viewport.height / 2,
  };
}

export function screenToWorld(point, camera, viewport) {
  const scale = camera.zoom * Math.min(viewport.width / camera.worldWidth, viewport.height / camera.worldHeight);
  return [
    (point[0] - viewport.width / 2) / scale + camera.x,
    (point[1] - viewport.height / 2) / scale + camera.y,
  ];
}

export function framebufferSize(cssWidth, cssHeight, dpr) {
  if (![cssWidth, cssHeight, dpr].every((value) => Number.isFinite(value) && value > 0)) {
    throw new RangeError(`invalid framebuffer inputs ${cssWidth}x${cssHeight} @ ${dpr}`);
  }
  return [Math.round(cssWidth * dpr), Math.round(cssHeight * dpr)];
}

export function canonicalFixtureBytes(fixture) {
  const encoder = new TextEncoder();
  const chunks = [];
  const addBytes = (bytes) => { const length = new Uint8Array(4); new DataView(length.buffer).setUint32(0, bytes.byteLength, true); chunks.push(length, bytes); };
  const addText = (value) => addBytes(encoder.encode(JSON.stringify(value)));
  const addFloat32 = (values) => { const bytes = new Uint8Array(values.length * 4); const view = new DataView(bytes.buffer); for (let i = 0; i < values.length; i += 1) view.setFloat32(i * 4, values[i], true); addBytes(bytes); };
  addText({ schema: 'rfatlas-renderer-canonical-fixture-v1', provenance: fixture.provenance });
  addText(fixture.tile);
  addText(fixture.floors);
  addText(fixture.floorplan.rooms);
  addText(fixture.floorplan.walls);
  addText(fixture.layers.map(({ id, width, height }) => ({ id, width, height })));
  for (const layer of fixture.layers) { addFloat32(layer.values); addBytes(layer.mask); }
  addFloat32(fixture.overlays.aps);
  addFloat32(fixture.overlays.paths);
  const total = chunks.reduce((sum, bytes) => sum + bytes.byteLength, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const bytes of chunks) { result.set(bytes, offset); offset += bytes.byteLength; }
  return result;
}

export function sampleLayer(layer, x, y) {
  if (!Number.isInteger(x) || !Number.isInteger(y) || x < 0 || y < 0 || x >= layer.width || y >= layer.height) {
    throw new RangeError(`layer coordinate (${x},${y}) is outside ${layer.width}x${layer.height}`);
  }
  const index = y * layer.width + x;
  return { value: layer.mask[index] === 0 ? null : layer.values[index], mask: layer.mask[index], index };
}

export function verifyFixture(fixture) {
  const layer = fixture.layers[0];
  const points = [[0, 0], [17, 22], [128, 96], [255, 191]];
  const samples = points.map(([x, y]) => ({ x, y, ...sampleLayer(layer, x, y) }));
  const camera = { x: 2048, y: 1536, zoom: 1.75, worldWidth: 4096, worldHeight: 3072 };
  const viewport = { width: 1200, height: 720 };
  const source = [731.25, 1412.5];
  const projected = worldToScreen(source, camera, viewport);
  const roundTrip = screenToWorld([projected.x, projected.y], camera, viewport);
  const roundTripError = Math.hypot(roundTrip[0] - source[0], roundTrip[1] - source[1]);
  return {
    coordinateRoundTripError: roundTripError,
    axisDirection: FIXTURE_PROVENANCE.world.yDirection,
    sampledCells: samples,
    unknownCellsAreNull: samples.filter((sample) => sample.mask === 0).every((sample) => sample.value === null),
    wallCount: fixture.floorplan.walls.length,
    layerCount: fixture.layers.length,
    overlayCount: fixture.overlays.count,
    floorCount: fixture.floors.length,
    fixtureHash: 'sha256-canonical-fixture-bytes',
    fixtureHashAlgorithm: 'SHA-256 over canonicalFixtureBytes(fixture)',
  };
}

export function makeTileData(z, x, y, size = 256) {
  const pixels = new Uint8Array(size * size * 4);
  const seed = hash32(z * 0x9e3779b1 ^ x * 0x85ebca6b ^ y * 0xc2b2ae35 ^ FIXTURE_PROVENANCE.seed);
  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      const i = (py * size + px) * 4;
      const stripe = ((px + x * 31) ^ (py + y * 17)) & 31;
      pixels[i] = (seed + px + z * 13) & 255;
      pixels[i + 1] = (seed + py + y * 19) & 255;
      pixels[i + 2] = stripe < 2 ? 255 : 92;
      pixels[i + 3] = 255;
    }
  }
  return pixels;
}

export function createCamera(fixture) {
  return { x: fixture.provenance.world.width / 2, y: fixture.provenance.world.height / 2, zoom: 1, worldWidth: fixture.provenance.world.width, worldHeight: fixture.provenance.world.height, bearing: 0, pitch: 0 };
}
