import { canonicalFixtureBytes, createCamera, FIXTURE_PROVENANCE, framebufferSize, makeFixture, makeTileData, sampleLayer, screenToWorld, WORLD_MILLIMETRES_PER_METRE, worldToScreen } from './fixture.js';

const fixture = makeFixture();
const state = {
  candidate: 'custom',
  workload: 'numeric',
  layerIndex: 0,
  camera: createCamera(fixture),
  viewport: { width: 1, height: 1, dpr: 1 },
  renderer: null,
  renderers: new Map(),
  benchmark: null,
  fixtureHash: null,
  canonicalFixtureByteLength: 0,
};

const els = {};

function byId(id) { return document.getElementById(id); }

function formatMs(value) { return Number.isFinite(value) ? `${value.toFixed(2)} ms` : 'unknown'; }
function formatBytes(value) {
  if (!Number.isFinite(value)) return 'unknown';
  if (value < 1024) return `${value.toFixed(0)} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 / 1024).toFixed(1)} MiB`;
}
function percentile(values, p) {
  if (!values.length) return Number.NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];
}
function nextFrame() { return new Promise((resolve) => requestAnimationFrame(resolve)); }

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function cellWorldPoint(cell) {
  return [(cell.x + 0.5) * fixture.provenance.world.width / 256, (cell.y + 0.5) * fixture.provenance.world.height / 192];
}

function framebufferPoint(point, camera, viewport) {
  const screen = worldToScreen(point, camera, viewport);
  return { css: [screen.x, screen.y], pixel: [Math.round(screen.x * viewport.dpr), Math.round((viewport.height - screen.y) * viewport.dpr)] };
}

function pixelHasInk(pixel) { return pixel[3] > 12 && (pixel[0] + pixel[1] + pixel[2]) > 18; }
function pixelLooksKnown(pixel) { return pixel[0] > 70; }
function pixelLooksUnknown(pixel) { return pixel[0] <= 110 && pixel[1] < 100 && pixel[3] > 20; }
function pixelLooksGeometry(pixel) { return pixelHasInk(pixel) && pixel[0] > 80; }

function normalize3(vector) {
  const length = Math.hypot(...vector) || 1;
  return vector.map((value) => value / length);
}

function cross3(a, b) { return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]; }
function dot3(a, b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }

/** Column-major perspective * look-at matrix shared by the 3D draw and probe. */
function perspectiveLookAt(aspect) {
  const eye = [2.7, 2.3, 3.4];
  const center = [0, -0.25, 0.05];
  const forward = normalize3([eye[0] - center[0], eye[1] - center[1], eye[2] - center[2]]);
  const right = normalize3(cross3([0, 1, 0], forward));
  const up = cross3(forward, right);
  const view = new Float32Array([
    right[0], up[0], forward[0], 0,
    right[1], up[1], forward[1], 0,
    right[2], up[2], forward[2], 0,
    -dot3(right, eye), -dot3(up, eye), -dot3(forward, eye), 1,
  ]);
  const f = 1 / Math.tan(0.95 / 2);
  const near = 0.1;
  const far = 10;
  const perspective = new Float32Array([
    f / Math.max(aspect, 0.01), 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) / (near - far), -1,
    0, 0, (2 * far * near) / (near - far), 0,
  ]);
  const matrix = new Float32Array(16);
  for (let column = 0; column < 4; column += 1) for (let row = 0; row < 4; row += 1) {
    matrix[column * 4 + row] = perspective[row] * view[column * 4] + perspective[4 + row] * view[column * 4 + 1] + perspective[8 + row] * view[column * 4 + 2] + perspective[12 + row] * view[column * 4 + 3];
  }
  return { matrix, eye, center, projection: 'perspective-lookAt-z-sensitive' };
}

export function classifyFloorPixels(pixels, colors, tolerance = 28) {
  const counts = colors.map(() => 0);
  for (let index = 0; index + 3 < pixels.length; index += 4) {
    if (pixels[index + 3] < 200) continue;
    let best = -1;
    let distance = Number.POSITIVE_INFINITY;
    for (let floor = 0; floor < colors.length; floor += 1) {
      const color = colors[floor];
      const candidateDistance = Math.abs(pixels[index] - color[0]) + Math.abs(pixels[index + 1] - color[1]) + Math.abs(pixels[index + 2] - color[2]);
      if (candidateDistance < distance) { distance = candidateDistance; best = floor; }
    }
    if (best >= 0 && distance <= tolerance) counts[best] += 1;
  }
  return counts;
}

class TileCache {
  constructor(capacity, viewportConcurrency = 3) {
    this.capacity = capacity;
    this.viewportConcurrency = viewportConcurrency;
    this.entries = new Map();
    this.pending = new Map();
    this.stats = { requests: 0, hits: 0, misses: 0, evictions: 0, cancelled: 0, viewportRequests: 0, viewportCompleted: 0, viewportCancelled: 0, viewportPeakActive: 0 };
    this.viewportController = null;
    this.viewportPromise = Promise.resolve();
    this.lastViewport = null;
  }
  key(z, x, y) { return `${z}/${x}/${y}`; }
  request(z, x, y, signal) {
    const key = this.key(z, x, y);
    this.stats.requests += 1;
    if (this.entries.has(key)) {
      this.stats.hits += 1;
      const entry = this.entries.get(key);
      this.entries.delete(key);
      this.entries.set(key, entry);
      return Promise.resolve(entry.data);
    }
    if (this.pending.has(key)) return this.pending.get(key);
    this.stats.misses += 1;
    const promise = new Promise((resolve, reject) => {
      let settled = false;
      const timer = setTimeout(() => {
        settled = true;
        signal?.removeEventListener('abort', abort);
        this.pending.delete(key);
        if (signal?.aborted) {
          this.stats.cancelled += 1;
          reject(new DOMException('tile request cancelled', 'AbortError'));
          return;
        }
        const data = makeTileData(z, x, y, fixture.tile.tileSize);
        this.entries.set(key, { data, bytes: data.byteLength });
        while (this.entries.size > this.capacity) {
          const oldest = this.entries.keys().next().value;
          this.entries.delete(oldest);
          this.stats.evictions += 1;
        }
        resolve(data);
      }, 0);
      const abort = () => { if (settled) return; settled = true; clearTimeout(timer); this.pending.delete(key); this.stats.cancelled += 1; reject(new DOMException('tile request cancelled', 'AbortError')); };
      signal?.addEventListener('abort', abort, { once: true });
    });
    this.pending.set(key, promise);
    return promise;
  }
  async stream(count = 96) {
    const before = { ...this.stats };
    const started = performance.now();
    const jobs = [];
    for (let i = 0; i < count; i += 1) {
      const controller = new AbortController();
      if (i % 13 === 0) setTimeout(() => controller.abort(), 0);
      jobs.push(this.request(3 + (i % 3), i % 17, (i * 3) % 17, controller.signal).catch(() => null));
    }
    await Promise.all(jobs);
    return { duration: performance.now() - started, before, after: { ...this.stats }, entries: this.entries.size };
  }
  beginViewport(camera) {
    const zoom = Math.max(0, Math.min(fixture.tile.maxZoom, Math.round(camera.zoom + 2)));
    const grid = 2 ** zoom;
    const span = fixture.provenance.world.width / grid;
    const centerX = Math.max(0, Math.min(grid - 1, Math.floor(camera.x / span)));
    const centerY = Math.max(0, Math.min(grid - 1, Math.floor(camera.y / (fixture.provenance.world.height / grid))));
    const priority = [];
    for (let radius = 0; radius <= 1; radius += 1) {
      for (let dy = -radius; dy <= radius; dy += 1) for (let dx = -radius; dx <= radius; dx += 1) {
        if (Math.max(Math.abs(dx), Math.abs(dy)) !== radius) continue;
        const x = centerX + dx; const y = centerY + dy;
        if (x >= 0 && y >= 0 && x < grid && y < grid) priority.push([zoom, x, y]);
      }
    }
    this.viewportController?.abort();
    const controller = new AbortController(); this.viewportController = controller;
    const requestedAt = { x: camera.x, y: camera.y, zoom: camera.zoom };
    const viewportState = { camera: requestedAt, priority: priority.map(([z, x, y]) => this.key(z, x, y)), started: [], requested: priority.length, completed: 0, cancelled: 0, concurrencyCap: this.viewportConcurrency, peakActive: 0 };
    this.lastViewport = viewportState;
    this.stats.viewportRequests += priority.length;
    this.viewportPromise = new Promise((resolve) => {
      let cursor = 0;
      let active = 0;
      const launch = () => {
        while (active < this.viewportConcurrency && cursor < priority.length) {
          const [z, x, y] = priority[cursor++];
          viewportState.started.push(this.key(z, x, y));
          active += 1;
          viewportState.peakActive = Math.max(viewportState.peakActive, active);
          this.stats.viewportPeakActive = Math.max(this.stats.viewportPeakActive, active);
          this.request(z, x, y, controller.signal)
            .then(() => { viewportState.completed += 1; this.stats.viewportCompleted += 1; })
            .catch((error) => { if (error.name === 'AbortError') { viewportState.cancelled += 1; this.stats.viewportCancelled += 1; } })
            .finally(() => {
              active -= 1;
              if (cursor === priority.length && active === 0) resolve(viewportState);
              else launch();
            });
        }
      };
      launch();
    });
    return this.viewportPromise;
  }
  waitViewport() { return this.viewportPromise; }
}

class BaseRenderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.tileCache = new TileCache(fixture.tile.cacheCapacity);
    this.frameTimes = [];
    this.lastRender = null;
    this.status = { state: 'loading', detail: '' };
    this.lastTileCamera = null;
  }
  resize() {
    const rect = this.canvas.getBoundingClientRect();
    state.viewport = { width: Math.max(1, rect.width), height: Math.max(1, rect.height), dpr: window.devicePixelRatio || 1 };
    [this.canvas.width, this.canvas.height] = framebufferSize(state.viewport.width, state.viewport.height, state.viewport.dpr);
  }
  async benchmarkFrameCount(count = 60) {
    const values = [];
    for (let i = 0; i < count; i += 1) {
      const start = performance.now();
      this.render(state.workload);
      await nextFrame();
      values.push(performance.now() - start);
    }
    this.frameTimes = values;
    return values;
  }
  scheduleViewportTiles() {
    const camera = state.camera;
    if (!this.lastTileCamera || this.lastTileCamera.x !== camera.x || this.lastTileCamera.y !== camera.y || this.lastTileCamera.zoom !== camera.zoom) {
      this.lastTileCamera = { x: camera.x, y: camera.y, zoom: camera.zoom };
      this.tileCache.beginViewport(camera);
    }
  }
  async exerciseAllLayers() {
    const original = state.layerIndex;
    const rendered = [];
    const framebufferSamples = [];
    const samplePoint = framebufferPoint(cellWorldPoint(fixture.probes.knownCell), state.camera, state.viewport);
    const started = performance.now();
    const memoryBefore = performance.memory ? performance.memory.usedJSHeapSize : null;
    for (let index = 0; index < fixture.layers.length; index += 1) {
      state.layerIndex = index;
      this.render('numeric');
      const pixel = this.readPixel(samplePoint.pixel, 'known');
      framebufferSamples.push({ layerId: fixture.layers[index].id, framebuffer: samplePoint.pixel, pixel, visible: pixelHasInk(pixel) });
      await nextFrame();
      rendered.push(fixture.layers[index].id);
    }
    state.layerIndex = original;
    this.render(state.workload);
    return { requested: fixture.layers.length, rendered, framebufferSamples, framebufferSampleVisible: framebufferSamples.every((sample) => sample.visible), distinctFramebufferSamples: new Set(framebufferSamples.map((sample) => sample.pixel.join(','))).size, cacheEntries: this.numericTextures?.size ?? null, duration: performance.now() - started, memoryBefore, memoryAfter: performance.memory ? performance.memory.usedJSHeapSize : null };
  }
  readPixel() { return [0, 0, 0, 0]; }
  framebufferProbe(workload = 'all') {
    const camera = { ...state.camera };
    const viewport = { ...state.viewport };
    const knownPoint = cellWorldPoint(fixture.probes.knownCell);
    const unknownPoint = cellWorldPoint(fixture.probes.unknownCell);
    const wall = fixture.probes.wallPoint;
    const wallPoint = [(wall.x1 + wall.x2) / 2, (wall.y1 + wall.y2) / 2];
    const overlayPoint = [fixture.probes.overlayPoint.x, fixture.probes.overlayPoint.y];
    const sample = (name, point, expected) => { const position = framebufferPoint(point, camera, viewport); const inViewport = position.css[0] >= 0 && position.css[0] < viewport.width && position.css[1] >= 0 && position.css[1] < viewport.height; const pixel = inViewport ? this.readPixel(position.pixel, expected) : [0, 0, 0, 0]; return { name, world: point, css: position.css, framebuffer: position.pixel, inViewport, pixel, visible: inViewport && pixelHasInk(pixel), expected, expectedColor: expected === 'known' ? 'numeric-red>70' : expected === 'unknown' ? 'unknown-red<=110-and-green<100' : 'geometry-ink' }; };
    const probes = [sample('known-cell', knownPoint, 'known'), sample('unknown-cell', unknownPoint, 'unknown'), sample('wall', wallPoint, 'geometry'), sample('overlay', overlayPoint, 'geometry')];
    const candidateProbe = workload === 'all' || workload === 'overlays' ? this.candidateGeometryProbe?.({ wallPoint, overlayPoint, probes, camera, viewport }) || null : null;
    const renderedCamera = this.lastRender?.camera;
    const sameCamera = renderedCamera && ['x', 'y', 'zoom'].every((key) => renderedCamera[key] === camera[key]);
    const rasterBound = this.lastRender?.rasterWorldBound === true;
    const vectorWallVisible = candidateProbe?.required ? candidateProbe.wall.colorMatch : probes[2].visible;
    const vectorOverlayVisible = candidateProbe?.required ? candidateProbe.overlay.colorMatch : probes[3].visible;
    return { workload, camera, viewportCss: [viewport.width, viewport.height], framebuffer: [this.canvas.width, this.canvas.height], probes, candidateProbe, masks: { knownCell: fixture.probes.knownCell, unknownCell: fixture.probes.unknownCell, knownPixelClass: pixelLooksKnown(probes[0].pixel), unknownPixelClass: pixelLooksUnknown(probes[1].pixel), knownVisible: probes[0].visible, unknownVisible: probes[1].visible }, alignment: { cameraBoundRaster: Boolean(sameCamera && rasterBound), overlayWorldPointsUseSameCamera: Boolean(sameCamera), wallWorldPointsUseSameCamera: Boolean(sameCamera), wallVisible: vectorWallVisible, overlayVisible: vectorOverlayVisible } };
  }
  candidateGeometryProbe() { return null; }
  probeThreeD() { return null; }
  unsupported(detail) {
    this.status = { state: 'unsupported', detail };
    renderStatus(this.status);
  }
  setStatus(detail) {
    this.status = { state: 'ready', detail };
    renderStatus(this.status);
  }
  render() {}
  destroy() {}
}

function makeProgram(gl, vertexSource, fragmentSource) {
  const compile = (type, source) => {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
    return shader;
  };
  const program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  return program;
}

function orthoCamera(camera, viewport) {
  const scale = camera.zoom * Math.min(viewport.width / camera.worldWidth, viewport.height / camera.worldHeight);
  const halfW = viewport.width / scale / 2;
  const halfH = viewport.height / scale / 2;
  return { left: camera.x - halfW, right: camera.x + halfW, top: camera.y - halfH, bottom: camera.y + halfH };
}

class CustomWebGLRenderer extends BaseRenderer {
  constructor(canvas) {
    super(canvas);
    this.gl = canvas.getContext('webgl2', { antialias: false, preserveDrawingBuffer: false, powerPreference: 'high-performance' });
    if (!this.gl) { this.unsupported('WebGL2 context unavailable; candidate is reported unsupported.'); return; }
    this.gl.getExtension('EXT_disjoint_timer_query_webgl2');
    this.programs = {};
    this.buffers = {};
    this.numericTextures = new Map();
    this.initGL();
    this.resize();
    this.setStatus('WebGL2 custom path; numeric values and unknown mask rendered from the shared typed-array fixture.');
  }
  initGL() {
    const gl = this.gl;
    this.programs.color = makeProgram(gl,
      `#version 300 es\n in vec2 a_position; uniform vec4 u_view; uniform vec4 u_color; out vec4 v_color;\n void main(){float x=(a_position.x-u_view.x)/(u_view.y-u_view.x)*2.0-1.0;float y=1.0-(a_position.y-u_view.z)/(u_view.w-u_view.z)*2.0;gl_Position=vec4(x,y,0,1);v_color=u_color;}`,
      `#version 300 es\n precision highp float; in vec4 v_color; out vec4 outColor; void main(){outColor=v_color;}`);
    this.programs.points = makeProgram(gl,
      `#version 300 es\n in vec2 a_position; in float a_value; uniform vec4 u_view; uniform float u_size; out float v_value;\n void main(){float x=(a_position.x-u_view.x)/(u_view.y-u_view.x)*2.0-1.0;float y=1.0-(a_position.y-u_view.z)/(u_view.w-u_view.z)*2.0;gl_Position=vec4(x,y,0,1);gl_PointSize=u_size;v_value=a_value;}`,
      `#version 300 es\n precision highp float; in float v_value; out vec4 outColor; void main(){vec2 p=gl_PointCoord*2.0-1.0;if(dot(p,p)>1.0)discard;float t=clamp((v_value+80.0)/80.0,0.0,1.0);outColor=vec4(t,0.25,1.0-t,0.8);}`);
    this.programs.numeric = makeProgram(gl,
      `#version 300 es\n in vec2 a_position; in vec2 a_uv; uniform vec4 u_view; out vec2 v_uv; void main(){float x=(a_position.x-u_view.x)/(u_view.y-u_view.x)*2.0-1.0;float y=1.0-(a_position.y-u_view.z)/(u_view.w-u_view.z)*2.0;gl_Position=vec4(x,y,0,1);v_uv=a_uv;}`,
      `#version 300 es\n precision highp float; uniform sampler2D u_values; in vec2 v_uv; out vec4 outColor; void main(){vec4 cell=texture(u_values,v_uv); if(cell.a<0.5){float hatch=mod(floor(gl_FragCoord.x/6.0)+floor(gl_FragCoord.y/6.0),2.0);outColor=vec4(0.15,0.17,0.22,0.72+0.18*hatch);}else{float t=clamp(cell.r,0.0,1.0);outColor=vec4(0.1+t*0.8,0.2+t*0.35,0.8-t*0.5,0.82);}}`);
    this.programs.floor3d = makeProgram(gl,
      `#version 300 es\n in vec3 a_position; uniform mat4 u_matrix; void main(){gl_Position=u_matrix*vec4(a_position,1);}`,
      `#version 300 es\n precision highp float; uniform vec4 u_color; out vec4 outColor; void main(){outColor=u_color;}`);
    this.buffers.walls = gl.createBuffer();
    this.buffers.aps = gl.createBuffer();
    this.buffers.paths = gl.createBuffer();
    this.buffers.quad = gl.createBuffer();
    this.buffers.numericQuad = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 1,-1, -1,1, 1,1]), gl.STATIC_DRAW);
  }
  resize() { super.resize(); if (this.gl) this.gl.viewport(0, 0, this.canvas.width, this.canvas.height); }
  viewUniform(program, view) {
    const gl = this.gl;
    gl.uniform4f(gl.getUniformLocation(program, 'u_view'), view.left, view.right, view.top, view.bottom);
  }
  drawColorLines(data, color, width = 1) {
    const gl = this.gl; const program = this.programs.color;
    gl.useProgram(program); gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.walls); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(data), gl.STREAM_DRAW);
    const pos = gl.getAttribLocation(program, 'a_position'); gl.enableVertexAttribArray(pos); gl.vertexAttribPointer(pos, 2, gl.FLOAT, false, 0, 0);
    this.viewUniform(program, orthoCamera(state.camera, state.viewport)); gl.uniform4f(gl.getUniformLocation(program, 'u_color'), ...color); gl.lineWidth(width); gl.drawArrays(gl.LINES, 0, data.length / 2);
  }
  drawOverlays(view) {
    const gl = this.gl; const { aps, paths, count } = fixture.overlays;
    const pointProgram = this.programs.points; gl.useProgram(pointProgram); gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.aps); gl.bufferData(gl.ARRAY_BUFFER, aps, gl.STREAM_DRAW);
    const pos = gl.getAttribLocation(pointProgram, 'a_position'); gl.enableVertexAttribArray(pos); gl.vertexAttribPointer(pos, 2, gl.FLOAT, false, 16, 0);
    const value = gl.getAttribLocation(pointProgram, 'a_value'); gl.enableVertexAttribArray(value); gl.vertexAttribPointer(value, 1, gl.FLOAT, false, 16, 8);
    this.viewUniform(pointProgram, view); gl.uniform1f(gl.getUniformLocation(pointProgram, 'u_size'), Math.max(2, 4 * state.viewport.dpr)); gl.drawArrays(gl.POINTS, 0, count);
    this.drawColorLines(Array.from(paths), [0.94, 0.76, 0.18, 0.35]);
  }
  numericTexture(layer) {
    const gl = this.gl; let texture = this.numericTextures.get(layer.id);
    if (!texture) { texture = gl.createTexture(); this.numericTextures.set(layer.id, texture); }
    const pixels = new Uint8Array(layer.values.length * 4);
    for (let i = 0; i < layer.values.length; i += 1) {
      const known = layer.mask[i] !== 0; const normalized = known ? Math.max(0, Math.min(1, (layer.values[i] + 85) / 55)) : 0;
      pixels[i * 4] = Math.round(normalized * 255); pixels[i * 4 + 1] = layer.mask[i] === 2 ? 180 : 80; pixels[i * 4 + 2] = 210; pixels[i * 4 + 3] = known ? 255 : 0;
    }
    gl.bindTexture(gl.TEXTURE_2D, texture); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, layer.width, layer.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    return texture;
  }
  drawNumeric(layer) {
    const gl = this.gl; const program = this.programs.numeric; gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffers.numericQuad); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, fixture.provenance.world.height, fixture.provenance.world.width, fixture.provenance.world.height, 0, 0, fixture.provenance.world.width, 0]), gl.STATIC_DRAW); const pos = gl.getAttribLocation(program, 'a_position'); gl.enableVertexAttribArray(pos); gl.vertexAttribPointer(pos, 2, gl.FLOAT, false, 0, 0);
    let uv = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, uv); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0,1, 1,1, 0,0, 1,0]), gl.STATIC_DRAW); const uvLoc = gl.getAttribLocation(program, 'a_uv'); gl.enableVertexAttribArray(uvLoc); gl.vertexAttribPointer(uvLoc, 2, gl.FLOAT, false, 0, 0);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, this.numericTexture(layer)); gl.uniform1i(gl.getUniformLocation(program, 'u_values'), 0); this.viewUniform(program, orthoCamera(state.camera, state.viewport)); gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4); gl.disableVertexAttribArray(uvLoc); gl.deleteBuffer(uv);
  }
  draw3d() {
    const gl = this.gl; const program = this.programs.floor3d; gl.useProgram(program); gl.enable(gl.DEPTH_TEST); gl.depthFunc(gl.LESS);
    const floorColors = [[0.95, 0.18, 0.18, 1], [0.95, 0.72, 0.12, 1], [0.18, 0.82, 0.3, 1], [0.12, 0.58, 0.98, 1], [0.62, 0.24, 0.94, 1], [0.98, 0.28, 0.68, 1]];
    const matrixInfo = perspectiveLookAt(this.canvas.width / Math.max(1, this.canvas.height));
    const loc = gl.getAttribLocation(program, 'a_position');
    const matrixLocation = gl.getUniformLocation(program, 'u_matrix');
    const colorLocation = gl.getUniformLocation(program, 'u_color');
    const verticesByFloor = [];
    const addFace = (vertices, a, b, c, d) => vertices.push(...a, ...b, ...c, ...a, ...c, ...d);
    for (const floor of fixture.floors) {
      const floorIndex = Number(floor.id.slice(-1)) - 1;
      // The source z values affect the camera-space depth; the x offset keeps each
      // floor independently visible for the framebuffer probe.
      const x = -1.15 + floorIndex * 0.44;
      const z = -0.8 + floorIndex * 0.22 + floor.z * 0.015;
      const y = -0.72 + floorIndex * 0.045;
      const w = 0.34;
      const d = 0.30;
      const h = floor.height * 0.82 + 0.12;
      const a = [x, y, z]; const b = [x + w, y, z]; const c = [x + w, y, z + d]; const d0 = [x, y, z + d];
      const e = [x, y + h, z]; const f = [x + w, y + h, z]; const g = [x + w, y + h, z + d]; const h0 = [x, y + h, z + d];
      const vertices = [];
      addFace(vertices, e, f, g, h0); // top
      addFace(vertices, a, d0, h0, e); // left side
      addFace(vertices, b, f, g, c); // right side
      addFace(vertices, a, e, f, b); // front side
      addFace(vertices, d0, c, g, h0); // back side
      verticesByFloor.push(vertices);
    }
    this.threeDFloorColors = floorColors.map((color) => color.slice(0, 3).map((value) => Math.round(value * 255)));
    this.threeDGeometry = { projection: matrixInfo.projection, eye: matrixInfo.eye, center: matrixInfo.center, floorVertices: verticesByFloor.map((vertices) => vertices.length / 3), floorColors: this.threeDFloorColors };
    gl.uniformMatrix4fv(matrixLocation, false, matrixInfo.matrix);
    gl.enableVertexAttribArray(loc);
    for (let floorIndex = 0; floorIndex < verticesByFloor.length; floorIndex += 1) {
      const buffer = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(verticesByFloor[floorIndex]), gl.STREAM_DRAW);
      gl.vertexAttribPointer(loc, 3, gl.FLOAT, false, 0, 0);
      gl.uniform4f(colorLocation, ...floorColors[floorIndex]);
      gl.drawArrays(gl.TRIANGLES, 0, verticesByFloor[floorIndex].length / 3);
      gl.deleteBuffer(buffer);
    }
    gl.disableVertexAttribArray(loc);
    gl.disable(gl.DEPTH_TEST);
  }
  render(workload = state.workload) {
    if (!this.gl || this.status.state === 'unsupported') return;
    this.resize(); this.scheduleViewportTiles(); const gl = this.gl; gl.viewport(0, 0, this.canvas.width, this.canvas.height); gl.clearColor(0.025, 0.04, 0.07, 1); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT); gl.enable(gl.BLEND); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    if (workload === '3d') {
      // A dedicated 3D pass keeps the floor probe independent from 2D walls and
      // the raster background. OpenLayers reports this workload unsupported.
      this.draw3d();
    } else {
      if (workload === 'numeric' || workload === 'all') this.drawNumeric(fixture.layers[state.layerIndex]);
      const view = orthoCamera(state.camera, state.viewport); const walls = []; for (const wall of fixture.floorplan.walls) walls.push(wall.x1, wall.y1, wall.x2, wall.y2); this.drawColorLines(walls, [0.8, 0.86, 0.95, 0.95], 2);
      if (workload === 'overlays' || workload === 'all') this.drawOverlays(view);
    }
    this.lastRender = { candidate: 'custom-webgl2', workload, dpr: state.viewport.dpr, framebuffer: [this.canvas.width, this.canvas.height], layerId: fixture.layers[state.layerIndex].id, camera: { ...state.camera }, rasterWorldBound: workload === 'numeric' || workload === 'all', floorProjection: workload === '3d' ? this.threeDGeometry?.projection : null };
  }
  readPixel([x, y]) { const pixel = new Uint8Array(4); this.gl.readPixels(Math.max(0, Math.min(this.canvas.width - 1, x)), Math.max(0, Math.min(this.canvas.height - 1, y)), 1, 1, this.gl.RGBA, this.gl.UNSIGNED_BYTE, pixel); return Array.from(pixel); }
  probeThreeD() {
    if (!this.threeDGeometry) return { floors: fixture.floors.length, geometryVertices: 0, floorPixels: [], floorVisible: false, visible: false, projection: 'not-rendered', preserveDrawingBuffer: false };
    const pixels = new Uint8Array(this.canvas.width * this.canvas.height * 4);
    this.gl.readPixels(0, 0, this.canvas.width, this.canvas.height, this.gl.RGBA, this.gl.UNSIGNED_BYTE, pixels);
    const floorPixels = classifyFloorPixels(pixels, this.threeDGeometry.floorColors);
    return { floors: fixture.floors.length, geometryVertices: this.threeDGeometry.floorVertices.reduce((sum, count) => sum + count, 0), floorVertices: this.threeDGeometry.floorVertices, floorColors: this.threeDGeometry.floorColors, floorPixels, floorVisible: floorPixels.map((count) => count > 0), nonBackgroundSamples: floorPixels.reduce((sum, count) => sum + count, 0), visible: floorPixels.length === fixture.floors.length && floorPixels.every((count) => count > 0), projection: this.threeDGeometry.projection, eye: this.threeDGeometry.eye, center: this.threeDGeometry.center, probeOrigin: 'dedicated 3D framebuffer; no walls or 2D background', preserveDrawingBuffer: false };
  }
}

class OpenLayersRenderer extends BaseRenderer {
  constructor(canvas) {
    super(canvas); this.map = null; this.wallMap = null; this.overlayMap = null; this.mapTargets = []; this.modules = null; this.imageSource = null; this.numericLayer = null; this.wallLayer = null; this.overlayLayer = null; this.vectorCanvas = null; this.wallCanvas = null; this.overlayCanvas = null; this.vectorCanvasInfo = null; this.wallCanvasInfo = null; this.overlayCanvasInfo = null; this.vectorSourceCounts = null; this.status = { state: 'loading', detail: 'Loading pinned OpenLayers 10.10.0 package from local node_modules…' };
  }
  async init() {
    try {
      const [Map, View, Projection, ImageLayer, ImageCanvas, WebGLVectorLayer, VectorSource, Feature, LineString, Point] = await Promise.all([
        import('/node_modules/ol/Map.js'), import('/node_modules/ol/View.js'), import('/node_modules/ol/proj/Projection.js'), import('/node_modules/ol/layer/Image.js'), import('/node_modules/ol/source/ImageCanvas.js'), import('/node_modules/ol/layer/WebGLVector.js'), import('/node_modules/ol/source/Vector.js'), import('/node_modules/ol/Feature.js'), import('/node_modules/ol/geom/LineString.js'), import('/node_modules/ol/geom/Point.js'),
      ]).then((imports) => imports.map((item) => item.default || item));
      this.modules = { Map, View, Projection, ImageLayer, ImageCanvas, WebGLVectorLayer, VectorSource, Feature, LineString, Point };
      const { Projection: Proj } = this.modules; const projection = new Proj({ code: 'RF_ATLAS_LOCAL_M', units: 'm', extent: [0, 0, fixture.provenance.world.width / WORLD_MILLIMETRES_PER_METRE, fixture.provenance.world.height / WORLD_MILLIMETRES_PER_METRE] }); this.projection = projection;
      const { Map: MapClass, View: ViewClass } = this.modules;
      const target = (name, zIndex) => { const element = document.createElement('div'); element.className = `rfatlas-ol-${name}`; element.setAttribute('aria-hidden', 'true'); Object.assign(element.style, { position: 'absolute', inset: '0', zIndex: String(zIndex), pointerEvents: 'none' }); this.canvas.parentElement.append(element); this.mapTargets.push(element); return element; };
      const viewOptions = { projection, center: [2048 / WORLD_MILLIMETRES_PER_METRE, 1536 / WORLD_MILLIMETRES_PER_METRE], zoom: 1 };
      this.map = new MapClass({ target: target('raster', 0), layers: [], view: new ViewClass(viewOptions) });
      this.wallMap = new MapClass({ target: target('walls', 1), layers: [], controls: [], view: new ViewClass(viewOptions) });
      this.overlayMap = new MapClass({ target: target('overlays', 2), layers: [], controls: [], view: new ViewClass(viewOptions) });
      this.buildLayers(); this.map.renderSync(); this.wallMap.renderSync(); this.overlayMap.renderSync(); this.setStatus('OpenLayers 10.10.0; local projection and synthetic canvas/vector sources; no external tiles.');
    } catch (error) {
      this.unsupported(`OpenLayers 10.10.0 unavailable in local node_modules (${error.message}); no fallback is reported.`);
    }
  }
  buildLayers() {
    const { ImageCanvas, ImageLayer: ImageLayerClass, WebGLVectorLayer: WebGLVectorLayerClass, VectorSource: VectorSourceClass, Feature: FeatureClass, LineString: LineStringClass, Point: PointClass } = this.modules;
    const mapPoint = ([x, y]) => [x / WORLD_MILLIMETRES_PER_METRE, (fixture.provenance.world.height - y) / WORLD_MILLIMETRES_PER_METRE];
    const extent = [0, 0, fixture.provenance.world.width / WORLD_MILLIMETRES_PER_METRE, fixture.provenance.world.height / WORLD_MILLIMETRES_PER_METRE];
    const imageSource = new ImageCanvas({ projection: this.projection, ratio: 1, interpolate: false, canvasFunction: (requestedExtent, _resolution, _pixelRatio, size) => { const canvas = document.createElement('canvas'); canvas.width = size[0]; canvas.height = size[1]; const ctx = canvas.getContext('2d'); const layer = fixture.layers[state.layerIndex]; const image = ctx.createImageData(layer.width, layer.height); for (let i = 0; i < layer.values.length; i += 1) { const x = i % layer.width; const y = Math.floor(i / layer.width); const known = layer.mask[i] !== 0; const t = known ? Math.max(0, Math.min(1, (layer.values[i] + 85) / 55)) : 0; const hatch = ((x >> 3) + (y >> 3)) % 2 === 0; image.data[i * 4] = known ? Math.round(t * 220) : (hatch ? 34 : 18); image.data[i * 4 + 1] = known ? 70 : (hatch ? 42 : 24); image.data[i * 4 + 2] = known ? 200 : (hatch ? 58 : 36); image.data[i * 4 + 3] = known ? 208 : (hatch ? 224 : 190); } const scratch = document.createElement('canvas'); scratch.width = layer.width; scratch.height = layer.height; scratch.getContext('2d').putImageData(image, 0, 0); const worldWidth = fixture.provenance.world.width / WORLD_MILLIMETRES_PER_METRE; const worldHeight = fixture.provenance.world.height / WORLD_MILLIMETRES_PER_METRE; const sourceX = Math.max(0, requestedExtent[0] / worldWidth * layer.width); const sourceY = Math.max(0, (worldHeight - requestedExtent[3]) / worldHeight * layer.height); const sourceWidth = Math.min(layer.width - sourceX, (requestedExtent[2] - requestedExtent[0]) / worldWidth * layer.width); const sourceHeight = Math.min(layer.height - sourceY, (requestedExtent[3] - requestedExtent[1]) / worldHeight * layer.height); ctx.imageSmoothingEnabled = false; ctx.drawImage(scratch, sourceX, sourceY, sourceWidth, sourceHeight, 0, 0, canvas.width, canvas.height); return canvas; }, });
    this.imageSource = imageSource; this.numericLayer = new ImageLayerClass({ source: imageSource, extent, opacity: 0.9 }); this.map.addLayer(this.numericLayer);
    const wallSource = new VectorSourceClass(); const vectorSource = new VectorSourceClass(); const wallFeatures = []; for (const wall of fixture.floorplan.walls) wallFeatures.push(new FeatureClass({ geometry: new LineStringClass([mapPoint([wall.x1, wall.y1]), mapPoint([wall.x2, wall.y2])]), kind: 'wall', kindCode: 1 }));
    const pointFeatures = []; const pathFeatures = []; for (let i = 0; i < fixture.overlays.count; i += 1) { const a = fixture.overlays.aps; const p = fixture.overlays.paths; pointFeatures.push(new FeatureClass({ geometry: new PointClass(mapPoint([a[i * 4], a[i * 4 + 1]])), kind: 'ap', kindCode: 3 })); pathFeatures.push(new FeatureClass({ geometry: new LineStringClass([mapPoint([p[i * 4], p[i * 4 + 1]]), mapPoint([p[i * 4 + 2], p[i * 4 + 3]])]), kind: 'path', kindCode: 2 })); }
    // Keep the fixture sources deterministic while ordering paths first and
    // walls/points after them.  This makes the candidate-specific probes test
    // their source colors when dense synthetic paths overlap a floorplan wall.
    for (const feature of wallFeatures) wallSource.addFeature(feature); for (const feature of pathFeatures) vectorSource.addFeature(feature); for (const feature of pointFeatures) vectorSource.addFeature(feature);
    this.vectorSourceCounts = { walls: fixture.floorplan.walls.length, aps: fixture.overlays.count, paths: fixture.overlays.count, total: fixture.floorplan.walls.length + fixture.overlays.count * 2 };
    // OpenLayers color arrays use RGB bytes (alpha remains 0..1). Keeping the
    // bytes explicit makes the direct WebGLVector probe source/color-specific.
    // Keep walls and overlays in separate WebGLVector layers. Each probe can
    // then inspect the actual layer canvas and source color directly, without
    // merged-canvas or brightest-pixel ambiguity under dense path coverage.
    this.wallLayer = new WebGLVectorLayerClass({ source: wallSource, disableHitDetection: true, style: { 'stroke-color': [218, 176, 42, 0.9], 'stroke-width': 2.4 } }); this.wallMap.addLayer(this.wallLayer);
    this.overlayLayer = new WebGLVectorLayerClass({ source: vectorSource, disableHitDetection: true, style: { 'stroke-color': [98, 70, 190, 0.72], 'stroke-width': 1.2, 'circle-radius': 3, 'circle-fill-color': [82, 176, 255, 0.88], 'circle-stroke-color': [31, 51, 87, 0.9], 'circle-stroke-width': 1 } }); this.overlayMap.addLayer(this.overlayLayer);
  }
  resize() { super.resize(); this.map?.updateSize(); this.wallMap?.updateSize(); this.overlayMap?.updateSize(); }
  render(workload = state.workload) { if (!this.map || this.status.state === 'unsupported') return; this.resize(); this.scheduleViewportTiles(); const scale = state.camera.zoom * Math.min(state.viewport.width / fixture.provenance.world.width, state.viewport.height / fixture.provenance.world.height); const center = [state.camera.x / WORLD_MILLIMETRES_PER_METRE, (fixture.provenance.world.height - state.camera.y) / WORLD_MILLIMETRES_PER_METRE]; const resolution = 1 / (scale * WORLD_MILLIMETRES_PER_METRE); for (const map of [this.map, this.wallMap, this.overlayMap]) { map?.getView().setCenter(center); map?.getView().setResolution(resolution); } const showNumeric = workload === 'numeric' || workload === 'all'; const showGeometry = workload === 'overlays' || workload === 'all'; this.numericLayer?.setVisible(showNumeric); this.wallLayer?.setVisible(showGeometry); this.overlayLayer?.setVisible(showGeometry); if (showNumeric) this.imageSource?.changed(); this.map.renderSync(); this.wallMap.renderSync(); this.overlayMap.renderSync(); this.updateVectorCanvas(); this.lastRender = { candidate: 'openlayers-10.10.0', workload, dpr: state.viewport.dpr, layerId: fixture.layers[state.layerIndex].id, threeD: false, camera: { ...state.camera }, rasterWorldBound: showNumeric, worldUnits: 'metres (fixture millimetres / 1000)', vectorCanvas: this.vectorCanvasInfo }; }
  updateVectorCanvas() {
    const canvases = [...Array.from(this.map?.getViewport()?.querySelectorAll('canvas') || []), ...Array.from(this.wallMap?.getViewport()?.querySelectorAll('canvas') || []), ...Array.from(this.overlayMap?.getViewport()?.querySelectorAll('canvas') || [])];
    const rendererCanvas = (layer) => layer?.getRenderer?.()?.helper?.gl_?.canvas || null;
    this.wallCanvas = rendererCanvas(this.wallLayer) || Array.from(this.wallMap?.getViewport()?.querySelectorAll('canvas') || []).find((canvas) => canvas.width > 0 && canvas.getContext('webgl')) || null;
    this.overlayCanvas = rendererCanvas(this.overlayLayer) || Array.from(this.overlayMap?.getViewport()?.querySelectorAll('canvas') || []).find((canvas) => canvas.width > 0 && canvas.getContext('webgl')) || null;
    this.vectorCanvas = this.overlayCanvas || this.wallCanvas;
    const info = (canvas, sourceFeatures) => {
      if (!canvas) return null;
      const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
      return { kind: 'OpenLayers WebGLVector renderer canvas', context: gl === canvas.getContext('webgl2') ? 'webgl2' : 'webgl', width: canvas.width, height: canvas.height, sourceFeatures, readbackOrigin: 'bottom-left (WebGL readPixels)' };
    };
    this.wallCanvasInfo = info(this.wallCanvas, { walls: this.vectorSourceCounts?.walls ?? 0, total: this.vectorSourceCounts?.walls ?? 0 });
    this.overlayCanvasInfo = info(this.overlayCanvas, { aps: this.vectorSourceCounts?.aps ?? 0, paths: this.vectorSourceCounts?.paths ?? 0, total: (this.vectorSourceCounts?.aps ?? 0) + (this.vectorSourceCounts?.paths ?? 0) });
    this.vectorCanvasInfo = this.overlayCanvasInfo || this.wallCanvasInfo;
  }
  getWebGLContext() { this.updateVectorCanvas(); return this.vectorCanvas ? (this.vectorCanvas.getContext('webgl2') || this.vectorCanvas.getContext('webgl')) : null; }
  readVectorPixel([x, y]) {
    this.updateVectorCanvas();
    const canvas = this.vectorCanvas;
    if (!canvas) return [0, 0, 0, 0];
    const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
    const px = Math.max(0, Math.min(canvas.width - 1, Math.round((x / Math.max(1, this.canvas.width)) * canvas.width)));
    // framebufferPoint already produces bottom-left coordinates. WebGL readPixels
    // consumes that origin directly; applying a second flip would invert y.
    const py = Math.max(0, Math.min(canvas.height - 1, Math.round((y / Math.max(1, this.canvas.height)) * canvas.height)));
    const pixel = new Uint8Array(4);
    gl.readPixels(px, py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
    return Array.from(pixel);
  }
  readVectorCanvasPixel([x, y]) {
    this.updateVectorCanvas();
    const canvas = this.vectorCanvas;
    if (!canvas) return [0, 0, 0, 0];
    const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
    const px = Math.max(0, Math.min(canvas.width - 1, Math.round(x)));
    const py = Math.max(0, Math.min(canvas.height - 1, Math.round(y)));
    const pixel = new Uint8Array(4);
    gl.readPixels(px, py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
    return Array.from(pixel);
  }
  readVectorNeighborhood([x, y], expected) {
    let best = [0, 0, 0, 0];
    let bestDistance = Number.POSITIVE_INFINITY;
    for (let dy = -4; dy <= 4; dy += 1) for (let dx = -4; dx <= 4; dx += 1) {
      const pixel = this.readVectorPixel([x + dx, y + dy]);
      const distance = expected.reduce((sum, value, index) => sum + Math.abs(pixel[index] - value), 0);
      if (pixel[3] > 15 && distance < bestDistance) { best = pixel; bestDistance = distance; }
    }
    return { pixel: best, colorDistance: bestDistance };
  }
  readVectorCanvasNeighborhood(canvas, [x, y], expected) {
    let best = [0, 0, 0, 0];
    let bestDistance = Number.POSITIVE_INFINITY;
    if (!canvas) return { pixel: best, colorDistance: bestDistance };
    const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
    for (let dy = -4; dy <= 4; dy += 1) for (let dx = -4; dx <= 4; dx += 1) {
      const px = Math.max(0, Math.min(canvas.width - 1, Math.round(x + dx)));
      const py = Math.max(0, Math.min(canvas.height - 1, Math.round(y + dy)));
      const data = new Uint8Array(4); gl.readPixels(px, py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, data); const pixel = Array.from(data);
      const distance = expected.reduce((sum, value, index) => sum + Math.abs(pixel[index] - value), 0);
      if (pixel[3] > 15 && distance < bestDistance) { best = pixel; bestDistance = distance; }
    }
    return { pixel: best, colorDistance: bestDistance };
  }
  readRasterPixel([x, y]) {
    const canvas = Array.from(this.map?.getViewport()?.querySelectorAll('canvas') || []).find((item) => item.getContext('2d'));
    if (!canvas) return [0, 0, 0, 0];
    const context = canvas.getContext('2d');
    const px = Math.max(0, Math.min(canvas.width - 1, Math.round((x / Math.max(1, this.canvas.width)) * canvas.width)));
    const py = Math.max(0, Math.min(canvas.height - 1, Math.round((y / Math.max(1, this.canvas.height)) * canvas.height)));
    return Array.from(context.getImageData(px, Math.max(0, canvas.height - 1 - py), 1, 1).data);
  }
  readPixel([x, y], expected = 'geometry') { return expected === 'geometry' ? this.readVectorPixel([x, y]) : this.readRasterPixel([x, y]); }
  vectorFramebufferPoint(worldPoint) {
    const mapPoint = [worldPoint[0] / WORLD_MILLIMETRES_PER_METRE, (fixture.provenance.world.height - worldPoint[1]) / WORLD_MILLIMETRES_PER_METRE];
    const mapPixel = this.map.getPixelFromCoordinate(mapPoint);
    const mapSize = this.map.getSize();
    return { css: mapPixel, pixel: [Math.round(mapPixel[0] / mapSize[0] * this.vectorCanvas.width), Math.round((mapSize[1] - mapPixel[1]) / mapSize[1] * this.vectorCanvas.height)], inViewport: mapPixel[0] >= 0 && mapPixel[0] < mapSize[0] && mapPixel[1] >= 0 && mapPixel[1] < mapSize[1] };
  }
  candidateGeometryProbe({ wallPoint, overlayPoint, camera, viewport }) {
    this.updateVectorCanvas();
    const wallPosition = framebufferPoint(wallPoint, camera, viewport);
    const overlayPosition = framebufferPoint(overlayPoint, camera, viewport);
    // The vector renderer writes premultiplied wall coverage to its transparent
    // canvas. Probe a small deterministic neighborhood and compare to the
    // expected covered stroke color; no other canvas or brightest-pixel choice
    // participates in this assertion.
    const wallRgb = [218, 176, 42];
    const overlayRgb = [82, 176, 255];
    const wallVectorPosition = this.vectorFramebufferPoint(wallPoint);
    const overlayVectorPosition = this.vectorFramebufferPoint(overlayPoint);
    const wallSample = wallVectorPosition.inViewport ? this.readVectorCanvasNeighborhood(this.wallCanvas, wallVectorPosition.pixel, wallRgb) : { pixel: [0, 0, 0, 0], colorDistance: Number.POSITIVE_INFINITY };
    const overlaySample = overlayVectorPosition.inViewport ? this.readVectorCanvasNeighborhood(this.overlayCanvas, overlayVectorPosition.pixel, overlayRgb) : { pixel: [0, 0, 0, 0], colorDistance: Number.POSITIVE_INFINITY };
    const matches = (sample, tolerance = 48) => sample.pixel[3] > 15 && sample.colorDistance <= tolerance;
    const negativeExpected = [255, 0, 255];
    const negativeWall = { pixel: wallSample.pixel, colorDistance: negativeExpected.reduce((sum, value, index) => sum + Math.abs(wallSample.pixel[index] - value), 0) };
    return { required: true, separateCanvases: Boolean(this.wallCanvas && this.overlayCanvas && this.wallCanvas !== this.overlayCanvas), canvas: this.vectorCanvasInfo, canvases: { wall: this.wallCanvasInfo, overlay: this.overlayCanvasInfo }, sourceFeatures: this.vectorSourceCounts, wall: { framebuffer: wallVectorPosition.pixel, mapCss: wallVectorPosition.css, sharedFramebuffer: wallPosition.pixel, pixel: wallSample.pixel, colorDistance: wallSample.colorDistance, source: 'floorplan wall LineString', canvas: this.wallCanvasInfo, expectedRgb: wallRgb, colorMatch: matches(wallSample) }, overlay: { framebuffer: overlayVectorPosition.pixel, mapCss: overlayVectorPosition.css, sharedFramebuffer: overlayPosition.pixel, pixel: overlaySample.pixel, source: 'AP Point feature', canvas: this.overlayCanvasInfo, colorDistance: overlaySample.colorDistance, expectedRgb: overlayRgb, colorMatch: matches(overlaySample) }, negativeControl: { expectedRgb: negativeExpected, colorMatch: matches(negativeWall), description: 'wall sample must not match an impossible magenta vector color' }, readbackOrigin: 'bottom-left (WebGL readPixels)' };
  }
  destroy() { this.map?.setTarget(null); this.wallMap?.setTarget(null); this.overlayMap?.setTarget(null); for (const target of this.mapTargets) target.remove(); this.mapTargets = []; }
}

function renderStatus(status) { els.status.textContent = `${status.state.toUpperCase()}: ${status.detail}`; els.status.dataset.state = status.state; }
function renderStats(stats) { els.stats.textContent = JSON.stringify(stats, null, 2); }
function renderFixtureInfo() { els.fixture.textContent = JSON.stringify({ ...FIXTURE_PROVENANCE, hash: state.fixtureHash ? `sha256-${state.fixtureHash}` : 'sha256-pending', probes: fixture.probes, checks: fixture.checks, workloads: { raster: 'synthetic local canvas', numericalLayers: fixture.layers.length, unknownMask: 'mask=0 renders as hatched/unknown', overlays: fixture.overlays.count, tiles: fixture.tile, floors3d: fixture.floors.length } }, null, 2); }

async function getRenderer(candidate = state.candidate) {
  if (state.renderers.has(candidate)) return state.renderers.get(candidate);
  const renderer = candidate === 'custom' ? new CustomWebGLRenderer(els.canvas) : new OpenLayersRenderer(els.canvas);
  state.renderers.set(candidate, renderer); state.renderer = renderer;
  if (renderer.init) await renderer.init();
  return renderer;
}

async function selectCandidate() {
  const nextCandidate = els.candidate.value;
  if (state.renderer && state.candidate === nextCandidate) {
    state.renderer.render(state.workload);
    renderStatus(state.renderer.status);
    return;
  }
  if (state.renderer) {
    state.renderer.destroy();
    // Keep the custom context alive while OpenLayers is exercised so switching
    // candidates does not manufacture a second WebGL context on one canvas.
    if (state.candidate === 'openlayers') state.renderers.delete('openlayers');
  }
  state.candidate = nextCandidate;
  els.canvas.style.visibility = state.candidate === 'custom' ? 'visible' : 'hidden';
  state.renderer = await getRenderer(state.candidate);
  state.renderer.render(state.workload);
  renderStatus(state.renderer.status);
}

async function runBenchmark() {
  const renderer = await getRenderer(state.candidate);
  if (renderer.status.state !== 'ready') {
    const candidate = state.candidate === 'custom' ? 'custom-webgl2' : 'openlayers-10.10.0';
    const unsupported3d = state.candidate === 'openlayers';
    const result = { schema: 'renderer-proof-v2', candidate, status: renderer.status.state, fixture: fixture.provenance, workload: { requested: state.workload, layerCount: fixture.layers.length, overlays: fixture.overlays.count, floorplanWalls: fixture.floorplan.walls.length, tilesRequested: 0, threeDFloors: fixture.floors.length }, layerExercise: null, environment: { userAgent: navigator.userAgent, platform: navigator.platform, viewportCss: [state.viewport.width, state.viewport.height], framebuffer: [els.canvas.width, els.canvas.height], dpr: state.viewport.dpr, webgl2: false, gpu: 'unavailable', vendor: 'unavailable', timerQuery: false, memoryApi: Boolean(performance.memory), preserveDrawingBuffer: false, worldUnit: 'fixture mm; OpenLayers map units m (mm / 1000)' }, latencyMs: null, tileCache: null, memory: null, support: { twoD: false, threeD: unsupported3d ? 'unsupported-by-this-candidate' : 'unsupported-by-runtime', unsupportedReason: renderer.status.detail }, correctness: { coordinateRoundTripError: fixture.checks.coordinateRoundTripError, unknownCellsAreNull: fixture.checks.unknownCellsAreNull, sampledCells: fixture.checks.sampledCells, selectedLayer: fixture.layers[state.layerIndex].id, framebufferProbes: [], threeDProbe: null, cameraSequence: [] } };
    state.benchmark = result; renderStats(result); els.download.disabled = false; els.run.disabled = false; return result;
  }
  els.run.disabled = true;
  const layerExercise = await renderer.exerciseAllLayers();
  const started = performance.now(); renderer.render(state.workload); await nextFrame(); const loadStart = performance.now(); renderer.render(state.workload); await nextFrame(); const load = performance.now() - loadStart;
  const original = { ...state.camera }; const panZoom = [];
  for (let i = 0; i < 40; i += 1) { state.camera.x = 700 + ((i * 173) % 2700); state.camera.y = 550 + ((i * 137) % 1800); state.camera.zoom = 0.55 + (i % 8) * 0.23; const t = performance.now(); renderer.render(state.workload); await nextFrame(); panZoom.push(performance.now() - t); }
  const framebufferProbes = [];
  for (const camera of [original, { ...original, x: 950, y: 870, zoom: 1.35 }, { ...original, x: 3150, y: 2220, zoom: 2.4 }]) { state.camera = { ...camera }; renderer.render('numeric'); const numeric = renderer.framebufferProbe('numeric'); renderer.render('all'); const composed = renderer.framebufferProbe('all'); framebufferProbes.push({ camera: { ...camera }, numeric, composed }); }
  state.camera = original; renderer.render(state.workload); const frames = await renderer.benchmarkFrameCount(50); const viewportTile = await renderer.tileCache.waitViewport(); const tiles = await renderer.tileCache.stream(96); state.camera = original; renderer.render('3d'); await nextFrame(); const threeDProbe = renderer.probeThreeD(); renderer.render(state.workload); const memory = performance.memory ? { usedJSHeapSize: performance.memory.usedJSHeapSize, totalJSHeapSize: performance.memory.totalJSHeapSize, limit: performance.memory.jsHeapSizeLimit } : null;
  const gl = renderer.gl || renderer.getWebGLContext?.(); const timerExtension = gl?.getExtension('EXT_disjoint_timer_query_webgl2'); const unsupported3d = state.candidate === 'openlayers'; const result = { schema: 'renderer-proof-v2', candidate: state.candidate === 'custom' ? 'custom-webgl2' : 'openlayers-10.10.0', status: renderer.status.state, fixture: fixture.provenance, workload: { requested: state.workload, layerCount: fixture.layers.length, overlays: fixture.overlays.count, floorplanWalls: fixture.floorplan.walls.length, tilesRequested: 96, threeDFloors: fixture.floors.length }, layerExercise, environment: { userAgent: navigator.userAgent, platform: navigator.platform, viewportCss: [state.viewport.width, state.viewport.height], framebuffer: [els.canvas.width, els.canvas.height], dpr: state.viewport.dpr, webgl2: Boolean(gl), gpu: gl ? (gl.getParameter(gl.RENDERER) || 'hidden') : 'unknown', vendor: gl ? (gl.getParameter(gl.VENDOR) || 'hidden') : 'unknown', timerQuery: Boolean(timerExtension), memoryApi: Boolean(memory), preserveDrawingBuffer: false, worldUnit: 'fixture mm; OpenLayers map units m (mm / 1000)' }, latencyMs: { load, panZoomP50: percentile(panZoom, 0.5), panZoomP95: percentile(panZoom, 0.95), frameP50: percentile(frames, 0.5), frameP95: percentile(frames, 0.95), tileStream: tiles.duration, total: performance.now() - started }, tileCache: { viewport: viewportTile, stress: tiles }, memory, support: { twoD: true, threeD: unsupported3d ? 'unsupported-by-this-candidate' : 'custom-floor-extrusion-proof-only', unsupportedReason: unsupported3d ? 'OpenLayers candidate is exercised as a 2D mapping stack; this harness does not silently claim native 3D floors.' : null }, correctness: { coordinateRoundTripError: fixture.checks.coordinateRoundTripError, unknownCellsAreNull: fixture.checks.unknownCellsAreNull, sampledCells: fixture.checks.sampledCells, selectedLayer: fixture.layers[state.layerIndex].id, framebufferProbes, threeDProbe, cameraSequence: framebufferProbes.map((probe) => probe.camera) } };
  state.benchmark = result; renderStats(result); els.download.disabled = false; els.download.onclick = () => downloadJson('renderer-proof-result.json', result); els.run.disabled = false; return result;
}

function downloadJson(name, data) { const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }); const url = URL.createObjectURL(blob); const link = document.createElement('a'); link.href = url; link.download = name; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }

function moveCamera(dx, dy, zoomDelta = 0) { state.camera.x = Math.max(0, Math.min(fixture.provenance.world.width, state.camera.x + dx)); state.camera.y = Math.max(0, Math.min(fixture.provenance.world.height, state.camera.y + dy)); state.camera.zoom = Math.max(0.25, Math.min(8, state.camera.zoom + zoomDelta)); state.renderer?.render(state.workload); updateCoordinateReadout(); }
function updateCoordinateReadout() { const rect = els.canvas.getBoundingClientRect(); const world = screenToWorld([rect.width / 2, rect.height / 2], state.camera, { width: rect.width, height: rect.height }); const cellX = Math.max(0, Math.min(255, Math.floor((world[0] / fixture.provenance.world.width) * 256))); const cellY = Math.max(0, Math.min(191, Math.floor((world[1] / fixture.provenance.world.height) * 192))); const sample = sampleLayer(fixture.layers[state.layerIndex], cellX, cellY); els.coordinate.textContent = `center world=(${world[0].toFixed(2)}, ${world[1].toFixed(2)}) mm · layer=${fixture.layers[state.layerIndex].id} · cell=(${cellX},${cellY}) · ${sample.mask === 0 ? 'UNKNOWN' : `value=${sample.value.toFixed(2)} dBm mask=${sample.mask}`}`; }

async function boot() {
  Object.assign(els, { canvas: byId('map'), candidate: byId('candidate'), workload: byId('workload'), layer: byId('layer'), status: byId('status'), stats: byId('stats'), fixture: byId('fixture'), coordinate: byId('coordinate'), run: byId('run'), download: byId('download') });
  els.layer.max = fixture.layers.length - 1; els.layer.value = '0'; els.layer.addEventListener('input', () => { state.layerIndex = Number(els.layer.value); state.renderer?.render(state.workload); updateCoordinateReadout(); }); els.candidate.addEventListener('change', selectCandidate); els.workload.addEventListener('change', () => { state.workload = els.workload.value; state.renderer?.render(state.workload); }); els.run.addEventListener('click', runBenchmark); byId('pan-left').addEventListener('click', () => moveCamera(-240, 0)); byId('pan-right').addEventListener('click', () => moveCamera(240, 0)); byId('pan-up').addEventListener('click', () => moveCamera(0, -180)); byId('pan-down').addEventListener('click', () => moveCamera(0, 180)); byId('zoom-in').addEventListener('click', () => moveCamera(0, 0, 0.35)); byId('zoom-out').addEventListener('click', () => moveCamera(0, 0, -0.35)); renderFixtureInfo(); updateCoordinateReadout(); const canonical = canonicalFixtureBytes(fixture); state.canonicalFixtureByteLength = canonical.byteLength; state.fixtureHash = await sha256Hex(canonical); renderFixtureInfo(); await selectCandidate(); state.renderer.render(state.workload); updateCoordinateReadout(); }

if (typeof document !== 'undefined') {
  window.__rfatlas = { fixture, state, makeFixture, runBenchmark, getRenderer, verify: () => fixture.checks };
  boot().catch((error) => { renderStatus({ state: 'error', detail: error.stack || error.message }); console.error(error); });
}

export { BaseRenderer, CustomWebGLRenderer, OpenLayersRenderer, TileCache };
