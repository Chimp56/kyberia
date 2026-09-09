const wasmUrl = new URL('./wasm/kyberia_scene_validator_wasm.wasm', import.meta.url);
const MAX_WORKER_ADMISSION_MS = 10_000;

let instancePromise;

async function instance(signal) {
  if (!instancePromise) {
    instancePromise = (async () => {
      if (!globalThis.WebAssembly) throw new Error('WebAssembly is unavailable');
      const response = await fetch(wasmUrl, { cache: 'no-store', signal });
      if (!response.ok) throw new Error(`scene validator request returned HTTP ${response.status}`);
      const bytes = await response.arrayBuffer();
      const result = await WebAssembly.instantiate(bytes, {});
      const exports = result.instance.exports;
      if (!exports.memory || !exports.input_ptr || !exports.validate_scene) {
        throw new Error('scene validator WASM exports are incomplete');
      }
      return result.instance;
    })();
  }
  return instancePromise;
}

self.onmessage = async ({ data }) => {
  const { id, bytes } = data || {};
  const deadlineMs = Number.isInteger(data?.deadlineMs) && data.deadlineMs > 0 && data.deadlineMs <= MAX_WORKER_ADMISSION_MS
    ? data.deadlineMs
    : MAX_WORKER_ADMISSION_MS;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), deadlineMs);
  try {
    if (!Number.isInteger(id) || !(bytes instanceof ArrayBuffer)) throw new Error('invalid scene validator request');
    const wasm = await instance(controller.signal);
    const input = new Uint8Array(bytes);
    const pointer = wasm.exports.input_ptr(input.byteLength);
    if (!pointer && input.byteLength !== 0) throw new Error('scene validator could not reserve input memory');
    new Uint8Array(wasm.exports.memory.buffer, pointer, input.byteLength).set(input);
    const status = wasm.exports.validate_scene(input.byteLength);
    self.postMessage({ id, status });
  } catch (error) {
    if (controller.signal.aborted) {
      self.postMessage({ id, code: 'timeout', error: `scene validator exceeded ${deadlineMs} ms` });
    } else {
      self.postMessage({ id, error: error instanceof Error ? error.message : String(error) });
    }
  } finally {
    clearTimeout(timeout);
  }
};
