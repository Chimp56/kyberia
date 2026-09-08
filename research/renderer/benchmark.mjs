#!/usr/bin/env node
/**
 * Browser proof runner.  It deliberately does not install or download a
 * browser: the host's Playwright installation is used when available.
 * Screenshots and JSON are supplied by the caller so retained evidence can be
 * kept beside this harness.  No external map or tile service is contacted.
 */
import { mkdir } from 'node:fs/promises';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname } from 'node:path';

const url = process.argv[2] || 'http://127.0.0.1:4173/index.html';
const output = process.argv[3] || 'evidence/browser-proof.json';
const screenshot = process.argv[4] || 'evidence/browser-proof.png';
const viewportText = process.env.RFATLAS_VIEWPORT || '1440x900';
const [viewportWidth, viewportHeight] = viewportText.split('x').map(Number);
const viewport = {
  width: Number.isFinite(viewportWidth) ? viewportWidth : 1440,
  height: Number.isFinite(viewportHeight) ? viewportHeight : 900,
};
const deviceScaleFactor = Number.isFinite(Number(process.env.RFATLAS_DPR)) ? Number(process.env.RFATLAS_DPR) : 1;

const sha256Bytes = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sha256Text = (value) => sha256Bytes(Buffer.from(value));
const sourceFiles = ['fixture.js', 'renderer.js', 'index.html', 'benchmark.mjs', 'fixture.test.mjs', 'package.json', 'package-inventory.mjs'];
const sourceDigests = {};
for (const file of sourceFiles) sourceDigests[file] = sha256Bytes(await readFile(file));
const lockDigest = sha256Bytes(await readFile('pnpm-lock.yaml'));
const playwrightPackage = JSON.parse(await readFile('node_modules/playwright/package.json', 'utf8'));

let playwright;
try {
  playwright = await import('playwright');
} catch (error) {
  console.error(JSON.stringify({ status: 'blocked', reason: 'Playwright is not installed in this worktree; Browser plugin was not available in the session.', error: error.message }, null, 2));
  process.exitCode = 2;
}
if (playwright) {
  const browser = await playwright.chromium.launch({ headless: true });
  await mkdir(dirname(output), { recursive: true });
  await mkdir(dirname(screenshot), { recursive: true });
  const page = await browser.newPage({ viewport, deviceScaleFactor });
  const consoleMessages = [];
  page.on('console', (message) => { if (message.type() === 'error' || message.type() === 'warning') consoleMessages.push({ type: message.type(), text: message.text() }); });
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(error.stack || error.message));
  await page.goto(url, { waitUntil: 'networkidle' });
  const identity = { title: await page.title(), url: page.url() };
  const dom = await page.locator('body').innerText();
  await page.waitForFunction(() => typeof window.__rfatlas?.state?.fixtureHash === 'string');
  const fixture = await page.evaluate(() => ({ checks: window.__rfatlas.verify(), probes: window.__rfatlas.fixture.probes, sha256: window.__rfatlas.state.fixtureHash, canonicalByteLength: window.__rfatlas.state.canonicalFixtureByteLength }));
  const screenshotFiles = [];
  const candidateResults = [];
  for (const candidate of ['custom', 'openlayers']) {
    await page.locator('#candidate').selectOption(candidate);
    await page.waitForFunction((expected) => {
      const current = window.__rfatlas?.state;
      const detail = current?.renderer?.status?.detail || '';
      return current?.candidate === expected && current.renderer && (current.renderer.status.state === 'unsupported' || (expected === 'custom' ? detail.startsWith('WebGL2') : detail.startsWith('OpenLayers')));
    }, candidate, { timeout: 30000 });
    for (const workload of ['numeric', 'overlays', 'all', '3d']) {
      await page.locator('#workload').selectOption(workload);
      await page.waitForFunction((expected) => window.__rfatlas?.state?.workload === expected, workload);
      await page.evaluate(() => { window.__rfatlas.state.benchmark = null; });
      await page.locator('#run').click();
      const expectedCandidate = candidate === 'custom' ? 'custom-webgl2' : 'openlayers-10.10.0';
      await page.waitForFunction(({ expectedCandidate: expected, expectedWorkload }) => {
        const result = window.__rfatlas?.state?.benchmark;
        return result?.candidate === expected && result?.workload?.requested === expectedWorkload;
      }, { expectedCandidate, expectedWorkload: workload }, { timeout: 120000 });
      const result = await page.evaluate(() => window.__rfatlas.state.benchmark);
      result.binding = { fixtureSha256: fixture.sha256 };
      if (result.status !== 'ready') {
        candidateResults.push(result);
        const unsupportedScreenshot = screenshot.replace(/\.png$/i, `-${candidate}-${workload}.png`);
        await page.screenshot({ path: unsupportedScreenshot, fullPage: false });
        screenshotFiles.push(unsupportedScreenshot);
        continue;
      }
      if (result.layerExercise?.rendered?.length !== fixture.checks.layerCount || result.layerExercise?.framebufferSamples?.length !== fixture.checks.layerCount || !result.layerExercise?.framebufferSampleVisible || result.layerExercise?.distinctFramebufferSamples < 2) throw new Error(`${candidate}/${workload} did not produce distinct visible framebuffer samples for all ${fixture.checks.layerCount} switched layers`);
      const frameProbes = result.correctness?.framebufferProbes || [];
      if (frameProbes.length !== 3) throw new Error(`${candidate}/${workload} missing camera framebuffer probes`);
      const baseNumeric = frameProbes[0].numeric;
      if (!baseNumeric.masks.knownVisible || !baseNumeric.masks.unknownVisible || !baseNumeric.masks.knownPixelClass || !baseNumeric.masks.unknownPixelClass) throw new Error(`${candidate}/${workload} failed candidate-specific numeric mask framebuffer probe`);
      if (frameProbes.some(({ composed }) => !composed.alignment.cameraBoundRaster || (composed.probes[2].inViewport && !composed.alignment.wallVisible) || (composed.probes[3].inViewport && !composed.alignment.overlayVisible))) throw new Error(`${candidate}/${workload} failed camera framebuffer alignment probe`);
      if (candidate === 'openlayers') {
        for (const { composed } of frameProbes) {
          const vector = composed.candidateProbe;
          if (!vector?.required || !vector.separateCanvases || vector.canvas?.kind !== 'OpenLayers WebGLVector renderer canvas' || vector.canvases?.wall?.kind !== 'OpenLayers WebGLVector renderer canvas' || vector.canvases?.overlay?.kind !== 'OpenLayers WebGLVector renderer canvas' || vector.readbackOrigin !== 'bottom-left (WebGL readPixels)') throw new Error(`${candidate}/${workload} did not identify separate direct WebGLVector canvases/readback origin`);
          if (vector.sourceFeatures?.walls !== fixture.checks.wallCount || vector.sourceFeatures?.aps !== fixture.checks.overlayCount || vector.sourceFeatures?.paths !== fixture.checks.overlayCount) throw new Error(`${candidate}/${workload} vector source counts do not match fixture`);
          if (composed.probes[2].inViewport && !vector.wall.colorMatch) throw new Error(`${candidate}/${workload} direct WebGLVector wall color probe failed`);
          if (composed.probes[3].inViewport && !vector.overlay.colorMatch) throw new Error(`${candidate}/${workload} direct WebGLVector AP color probe failed`);
          if (vector.negativeControl?.colorMatch !== false) throw new Error(`${candidate}/${workload} direct WebGLVector negative color control matched unexpectedly`);
        }
      }
      if (candidate === 'custom' && workload === '3d' && (!result.correctness.threeDProbe?.visible || !result.correctness.threeDProbe?.floorVisible?.every(Boolean) || !result.correctness.threeDProbe?.projection?.startsWith('perspective'))) throw new Error('custom 3D framebuffer probe found no per-floor perspective geometry');
      candidateResults.push(result);
      const candidateScreenshot = screenshot.replace(/\.png$/i, `-${candidate}-${workload}.png`);
      await page.screenshot({ path: candidateScreenshot, fullPage: false });
      screenshotFiles.push(candidateScreenshot);
    }
  }
  const resizeProbe = [];
  const resizedViewport = { width: Math.max(320, viewport.width - 240), height: Math.max(240, viewport.height - 180) };
  for (const candidate of ['custom', 'openlayers']) {
    await page.locator('#candidate').selectOption(candidate);
    await page.waitForFunction((expected) => {
      const current = window.__rfatlas?.state;
      const detail = current?.renderer?.status?.detail || '';
      return current?.candidate === expected && current.renderer && (current.renderer.status.state === 'unsupported' || (expected === 'custom' ? detail.startsWith('WebGL2') : detail.startsWith('OpenLayers')));
    }, candidate, { timeout: 30000 });
    await page.locator('#workload').selectOption('all');
    await page.waitForFunction(() => window.__rfatlas?.state?.workload === 'all');
    const before = await page.evaluate(() => ({ inner: [innerWidth, innerHeight], canvas: [document.querySelector('#map').width, document.querySelector('#map').height], dpr: devicePixelRatio }));
    await page.setViewportSize(resizedViewport);
    await page.waitForTimeout(50);
    const after = await page.evaluate(() => { window.__rfatlas.state.renderer?.render('all'); const canvas = document.querySelector('#map'); return { inner: [innerWidth, innerHeight], canvas: [canvas.width, canvas.height], dpr: devicePixelRatio }; });
    await page.setViewportSize(viewport);
    await page.waitForTimeout(50);
    await page.evaluate(() => window.__rfatlas.state.renderer?.render('all'));
    resizeProbe.push({ candidate, before, resized: after, restored: await page.evaluate(() => ({ inner: [innerWidth, innerHeight], canvas: [document.querySelector('#map').width, document.querySelector('#map').height], dpr: devicePixelRatio })) });
  }
  await page.screenshot({ path: screenshot, fullPage: false });
  screenshotFiles.push(screenshot);
  const browserIdentity = { userAgent: await page.evaluate(() => navigator.userAgent), platform: await page.evaluate(() => navigator.platform), playwright: playwrightPackage.version, chromium: playwright.chromium.executablePath(), viewport, deviceScaleFactor };
  const browserSha256 = sha256Text(JSON.stringify(browserIdentity));
  const screenshotDigests = Object.fromEntries(await Promise.all(screenshotFiles.map(async (file) => [file, sha256Bytes(await readFile(file))])));
  const report = { schema: 'renderer-browser-evidence-v2', binding: { fixtureSha256: fixture.sha256, sourceSha256: sha256Text(JSON.stringify(sourceDigests)), sourceFiles: sourceDigests, lockSha256: lockDigest, browserSha256, browser: browserIdentity, screenshots: screenshotDigests }, environment: { url, browser: browserIdentity.userAgent, platform: browserIdentity.platform, viewport: [viewport.width, viewport.height], dpr: deviceScaleFactor, browserPlugin: 'unavailable; regular Playwright fallback' }, identity, checks: { notBlank: dom.includes('Gate B renderer proof'), consoleErrors: consoleMessages, pageErrors, fixture }, resizeProbe, candidates: candidateResults };
  await writeFile(output, JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  await browser.close();
}
