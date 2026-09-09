import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright';

const baseUrl = process.argv[2] || 'http://127.0.0.1:4173/index.html';
const output = process.argv[3] || '.trash/browser-canonical-scenes';
await mkdir(output, { recursive: true });
const canonicalPath = new URL('./fixtures/canonical-scene-v1.json', import.meta.url);
const canonical = await readFile(canonicalPath, 'utf8');
const pointFixture = await readFile(new URL('./fixtures/canonical-scene-point-v1.json', import.meta.url), 'utf8');
const invalidPath = `${output}/duplicate-key.json`;
await writeFile(invalidPath, canonical.replace('{"schema":"V1"', '{"schema":"V1","schema":"V1"'));
const pointPath = `${output}/point-value.json`;
await writeFile(pointPath, pointFixture);
const escapedSchemaPath = `${output}/escaped-schema.json`;
await writeFile(escapedSchemaPath, canonical.replace('"schema":"V1"', '"schema":"\\u00561"'));
const floatWidthPath = `${output}/float-width.json`;
await writeFile(floatWidthPath, canonical.replace('"width":8', '"width":8.0'));

const browser = await chromium.launch({ headless: true });
const errors = [];
function assertDiscreteRasterProbe(probe, candidate) {
  assert.equal(probe.sampling, 'nearest-cell', `${candidate} must report discrete cell sampling`);
  const byName = new Map(probe.samples.map((sample) => [sample.name, sample]));
  const knownCenter = byName.get('known-center');
  const knownLeft = byName.get('known-near-left-edge');
  const knownRight = byName.get('known-near-right-edge');
  const unknownCenter = byName.get('unknown-center');
  const unknownEdge = byName.get('unknown-near-known-edge');
  for (const sample of [knownCenter, knownLeft, knownRight, unknownCenter, unknownEdge]) {
    assert.equal(sample.inViewport, true, `${candidate} ${sample.name} must be visible`);
    assert.equal(sample.known, sample.expected === 'known', `${candidate} ${sample.name} must use its canonical mask`);
    assert.equal(sample.pixelClass, true, `${candidate} ${sample.name} must retain its canonical pixel class`);
  }
  assert.equal(knownCenter.value, -42, `${candidate} center must preserve the canonical RSSI value`);
  assert.equal(unknownCenter.value, null, `${candidate} unknown neighbor must remain unknown`);
  const colorDistance = (left, right) => left.reduce((sum, value, index) => sum + Math.abs(value - right[index]), 0);
  assert.ok(colorDistance(knownCenter.pixel, knownLeft.pixel) <= 8, `${candidate} left edge must sample the same known cell as its center`);
  assert.ok(colorDistance(knownCenter.pixel, knownRight.pixel) <= 8, `${candidate} right edge must sample the same known cell as its center`);
}
async function openPage(viewport, screenshot) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: viewport.width < 500 ? 2 : 1 });
  const page = await context.newPage();
  page.on('console', (message) => { if (message.type() === 'error') errors.push(`console: ${message.text()}`); });
  page.on('pageerror', (error) => errors.push(`page: ${error.message}`));
  await page.goto(baseUrl, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => window.__rfatlas?.state?.scene !== null);
  const initial = await page.evaluate(() => ({
    status: document.querySelector('#status')?.dataset.state,
    worldBounds: window.__rfatlas.state.scene.layer.worldBounds,
    camera: { ...window.__rfatlas.state.camera },
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  assert.equal(initial.status, 'ready');
  assert.deepEqual(initial.worldBounds, [0, 0, 8, 6]);
  assert.equal(initial.scrollWidth, initial.innerWidth);
  const rasterProbe = await page.evaluate(() => {
    const renderer = window.__rfatlas.state.renderer;
    renderer.render('numeric');
    return renderer.numericRasterProbe();
  });
  assertDiscreteRasterProbe(rasterProbe, `custom ${viewport.width}px`);
  await page.screenshot({ path: `${output}/${screenshot}`, fullPage: true });
  await page.locator('#scene-file').setInputFiles(invalidPath);
  await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'invalid');
  const afterInvalid = await page.evaluate(() => ({ scene: window.__rfatlas.state.scene, rendererLast: window.__rfatlas.state.renderer?.lastRender }));
  assert.equal(afterInvalid.scene, null);
  assert.equal(afterInvalid.rendererLast, null);
  for (const alternate of [escapedSchemaPath, floatWidthPath]) {
    await page.locator('#scene-file').setInputFiles(alternate);
    await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'invalid');
    const afterAlternate = await page.evaluate(() => ({ scene: window.__rfatlas.state.scene, rendererLast: window.__rfatlas.state.renderer?.lastRender, detail: document.querySelector('#status')?.textContent }));
    assert.equal(afterAlternate.scene, null);
    assert.equal(afterAlternate.rendererLast, null);
    assert.match(afterAlternate.detail, /canonical-bytes|wasm-admission/);
  }
  await page.locator('#scene-file').setInputFiles(pointPath);
  await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'ready');
  const pointRasterProbe = await page.evaluate(() => {
    const renderer = window.__rfatlas.state.renderer;
    renderer.render('numeric');
    return renderer.numericRasterProbe();
  });
  assertDiscreteRasterProbe(pointRasterProbe, `custom PointValue ${viewport.width}px`);
  assert.equal(pointRasterProbe.layerId, 'wifi.rssi@wifi.rssi/1');
  await page.close();
  await context.close();
  return { initial, rasterProbe, pointRasterProbe, afterInvalid };
}

async function verifyDelayedBundledFetchCannotOverrideSynthetic(phase) {
  const context = await browser.newContext({ viewport: { width: 960, height: 640 } });
  const page = await context.newPage();
  const pageErrors = [];
  page.on('console', (message) => { if (message.type() === 'error') pageErrors.push(`console: ${message.text()}`); });
  page.on('pageerror', (error) => pageErrors.push(`page: ${error.message}`));
  if (phase === 'before-fetch') {
    await page.addInitScript(() => {
      const originalDigest = SubtleCrypto.prototype.digest;
      let held = true;
      const pending = [];
      Object.defineProperty(SubtleCrypto.prototype, 'digest', { configurable: true, writable: true, value(...args) {
        if (!held) return originalDigest.apply(this, args);
        return new Promise((resolve, reject) => pending.push(() => originalDigest.apply(this, args).then(resolve, reject)));
      } });
      window.__releaseInitialDigest = () => { held = false; while (pending.length) pending.shift()(); };
    });
  }
  let releaseFetch;
  const fetchHeld = new Promise((resolve) => { releaseFetch = resolve; });
  let routeEntered = false;
  let routeEnteredResolve;
  const routeStarted = new Promise((resolve) => { routeEnteredResolve = resolve; });
  await page.route('**/fixtures/canonical-scene-v1.json', async (route) => {
    routeEntered = true;
    routeEnteredResolve();
    await fetchHeld;
    await route.continue();
  });
  const navigation = page.goto(baseUrl, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#source');
  await page.waitForFunction(() => Boolean(window.__rfatlas?.state));
  if (phase === 'in-flight') await routeStarted;
  await page.selectOption('#source', 'synthetic');
  await page.waitForFunction(() => window.__rfatlas?.state?.source === 'synthetic' && window.__rfatlas.state.sceneStatus.state === 'ready');
  if (phase === 'before-fetch') {
    assert.equal(routeEntered, false, 'the pre-fetch case must select Synthetic before startup fetch begins');
    await page.evaluate(() => window.__releaseInitialDigest());
  } else {
    releaseFetch();
  }
  await navigation;
  await page.waitForTimeout(500);
  const result = await page.evaluate(() => ({ source: window.__rfatlas.state.source, scene: window.__rfatlas.state.scene, status: window.__rfatlas.state.sceneStatus }));
  assert.equal(result.source, 'synthetic', 'a stale bundled fetch must not change an explicit synthetic selection');
  assert.equal(result.scene, null, 'a stale bundled fetch must not install a canonical scene');
  assert.equal(result.status.state, 'ready');
  assert.match(result.status.detail, /synthetic Gate B stress fixture/);
  assert.deepEqual(pageErrors, []);
  await page.close();
  await context.close();
  return { phase, routeEntered, ...result };
}

const delayedBundledFetch = {
  beforeFetch: await verifyDelayedBundledFetchCannotOverrideSynthetic('before-fetch'),
  inFlight: await verifyDelayedBundledFetchCannotOverrideSynthetic('in-flight'),
};
const desktop = await openPage({ width: 1280, height: 900 }, 'desktop.png');
const mobile = await openPage({ width: 390, height: 844 }, 'mobile.png');
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await context.newPage();
page.on('console', (message) => { if (message.type() === 'error') errors.push(`openlayers console: ${message.text()}`); });
page.on('pageerror', (error) => errors.push(`openlayers page: ${error.message}`));
await page.goto(baseUrl, { waitUntil: 'networkidle' });
await page.waitForFunction(() => window.__rfatlas?.state?.scene !== null);
await page.selectOption('#candidate', 'openlayers');
await page.waitForTimeout(1_000);
const openLayers = await page.evaluate(() => ({ status: document.querySelector('#status')?.dataset.state, candidate: window.__rfatlas.state.candidate, rendererStatus: window.__rfatlas.state.renderer?.status, imageInterpolation: window.__rfatlas.state.renderer?.imageSource?.getInterpolate?.(), lastRender: window.__rfatlas.state.renderer?.lastRender }));
assert.equal(openLayers.candidate, 'openlayers');
assert.equal(openLayers.status, 'ready');
assert.equal(openLayers.imageInterpolation, false, 'OpenLayers ImageCanvas must keep discrete cell interpolation disabled');
assert.equal(openLayers.lastRender?.rasterWorldBound, true);
const openLayersRaster = await page.evaluate(() => {
  const renderer = window.__rfatlas.state.renderer;
  renderer.render('numeric');
  return renderer.numericRasterProbe();
});
assertDiscreteRasterProbe(openLayersRaster, 'openlayers');
openLayers.rasterProbe = openLayersRaster;
await page.screenshot({ path: `${output}/openlayers.png`, fullPage: true });
await page.close();
await context.close();
await browser.close();
assert.deepEqual(errors, []);
console.log(JSON.stringify({ baseUrl, output, delayedBundledFetch, desktop, mobile, openLayers, errors }, null, 2));
