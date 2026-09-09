/**
 * Browser-side admission for the versioned kyberia.render-scene wire format.
 *
 * This module deliberately has no renderer or framework dependency. It checks
 * the bounded JSON contract, preserves the source object, verifies the metric
 * artifact hash, and derives a display layer only after all checks succeed.
 * The Rust adapter remains the authority for canonical bytes and numerical
 * semantics; the browser repeats the inexpensive invariants before drawing.
 */

export const SCENE_CONTRACT_ID = 'kyberia.render-scene/1';
export const SCENE_WIRE_SCHEMA = 'V1';
export const MAX_SCENE_BYTES = 64 * 1024 * 1024;
export const MAX_SCENE_CELLS = 100_000;
export const MAX_SCENE_SAMPLES = 100_000;
export const MAX_SCENE_GROUPS = 100_000;
export const MAX_SCENE_CONTRIBUTIONS = MAX_SCENE_CELLS * 64;
export const MAX_SCENE_TEXT_BYTES = 1024;
export const MAX_METRIC_DEFINITION_BYTES = 16 * 1024;
export const MAX_WASM_ADMISSION_MS = 10_000;

const UNKNOWN_REASONS = new Set([
  'not_measured', 'not_advertised', 'not_observable', 'not_applicable',
  'unsupported_capability', 'permission_denied', 'filtered_out',
  'below_detection_threshold', 'failed_test', 'no_association',
  'invalid_geometry', 'solver_failure', 'outside_evidence_support',
  'clock_unavailable', 'source_did_not_provide', 'redacted', 'not_retained',
]);
const SCENE_CLASSES = new Set(['observed', 'interpolated', 'extrapolated', 'unknown']);
const AGGREGATION_METHODS = new Set(['median_dbm', 'linear_power_mean', 'trimmed_mean_dbm', 'percentile_range', 'ewma_dbm', 'robust_state_space_dbm']);

export class SceneLoadError extends Error {
  constructor(state, code, message) {
    super(message);
    this.name = 'SceneLoadError';
    this.state = state;
    this.code = code;
  }
}

function invalid(code, message) { throw new SceneLoadError('invalid', code, message); }
function resource(code, message) { throw new SceneLoadError('resource-limit', code, message); }
function unsupported(code, message) { throw new SceneLoadError('unsupported', code, message); }

function checkCancelled(isCancelled) {
  if (isCancelled?.()) throw new SceneLoadError('cancelled', 'cancelled', 'scene load cancelled');
}

function object(value, name, keys) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid('type', `${name} must be an object`);
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key, index) => key !== keys[index])) {
    invalid('schema-fields', `${name} has unknown or missing fields`);
  }
  return value;
}

function text(value, name, { empty = false } = {}) {
  if (typeof value !== 'string') invalid('type', `${name} must be text`);
  if (!empty && value.length === 0) invalid('value', `${name} cannot be empty`);
  if (value.length > MAX_SCENE_TEXT_BYTES || [...value].some((character) => character < ' ')) {
    resource('text', `${name} exceeds the bounded text contract`);
  }
  return value;
}

function finite(value, name) {
  if (typeof value !== 'number' || !Number.isFinite(value)) invalid('number', `${name} must be finite`);
  return value;
}

function integer(value, name, minimum, maximum) {
  finite(value, name);
  if (!Number.isInteger(value) || value < minimum || value > maximum) invalid('integer', `${name} is outside its bound`);
  return value;
}

function exactInteger(value, name, minimum = 0, maximum = Number.MAX_SAFE_INTEGER) {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) invalid('integer', `${name} must be an exact decimal string`);
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < minimum || parsed > maximum) resource('integer', `${name} is outside its bound`);
  return parsed;
}

function id(value, name) {
  text(value, name);
  if (!/^[0-9a-f]{32}$/.test(value)) invalid('identity', `${name} is not a canonical 128-bit ID`);
  return value;
}

function hash(value, name) {
  text(value, name);
  if (!/^[0-9a-f]{64}$/.test(value)) invalid('hash', `${name} is not a canonical SHA-256 hash`);
  return value;
}

function evidence(value, name, kind) {
  object(value, name, ['state', 'detail']);
  text(value.state, `${name}.state`);
  if (value.state === 'known') {
    if (kind === 'covariance') {
      if (!Array.isArray(value.detail) || value.detail.length !== 6) invalid('value', `${name}.detail must be a covariance vector`);
      value.detail.forEach((entry, index) => finite(entry, `${name}.detail[${index}]`));
    } else {
      finite(value.detail, `${name}.detail`);
    }
  } else if (value.state === 'unknown') {
    text(value.detail, `${name}.detail`);
    if (!UNKNOWN_REASONS.has(value.detail)) invalid('value', `${name}.detail is not an UnknownReason`);
  } else {
    invalid('value', `${name}.state is not Evidence::Known or Evidence::Unknown`);
  }
  return value;
}

function artifact(value, name) {
  object(value, name, ['version', 'sha256', 'byte_length', 'media_type']);
  text(value.version, `${name}.version`);
  hash(value.sha256, `${name}.sha256`);
  exactInteger(value.byte_length, `${name}.byte_length`, 0, MAX_SCENE_BYTES);
  text(value.media_type, `${name}.media_type`);
  return value;
}

function sourceArtifact(value, name) {
  object(value, name, ['sha256', 'media_type', 'byte_length']);
  hash(value.sha256, `${name}.sha256`);
  integer(value.byte_length, `${name}.byte_length`, 0, MAX_SCENE_BYTES);
  text(value.media_type, `${name}.media_type`);
  return value;
}

function point(value, name) {
  object(value, name, ['x', 'y']);
  finite(value.x, `${name}.x`);
  finite(value.y, `${name}.y`);
  return value;
}

function preflightJson(bytes, isCancelled) {
  // Rust emits compact serde_json with struct-field order. This finite scanner
  // enforces the parts JSON.parse intentionally discards: duplicate keys,
  // array cardinality, nesting, and non-canonical whitespace. It does not
  // construct the scene object, so hostile arrays are rejected before parser
  // allocations. JSON.parse remains responsible for complete JSON grammar.
  const stack = [];
  let rootSeen = false;
  let index = 0;
  let scanned = 0;
  const arrayLimit = (field) => {
    if (field === 'metric_definition_bytes') return MAX_METRIC_DEFINITION_BYTES;
    if (field === 'contributors') return 64;
    if (field === 'samples' || field === 'location_groups' || field === 'cells' || field === 'observation_ids' || field === 'observation_order') return MAX_SCENE_SAMPLES;
    return MAX_SCENE_CONTRIBUTIONS;
  };
  const consumeValue = () => {
    const frame = stack[stack.length - 1];
    if (!frame) {
      if (rootSeen) invalid('json', 'scene JSON contains more than one root value');
      rootSeen = true;
      return;
    }
    if (frame.kind === 'object') {
      if (frame.state !== 'value') invalid('json', 'scene JSON has an unexpected value');
      frame.state = 'comma';
      return;
    }
    if (frame.state !== 'value') invalid('json', 'scene JSON has an unexpected array value');
    frame.count += 1;
    if (frame.count > arrayLimit(frame.field)) resource('array-items', `${frame.field || 'scene'} array exceeds its preflight bound`);
    frame.state = 'comma';
  };
  const scanString = (start) => {
    let cursor = start + 1;
    let stringBytes = 0;
    while (cursor < bytes.length) {
      if ((cursor & 0x3ff) === 0) checkCancelled(isCancelled);
      const current = bytes[cursor];
      if (current === 0x22) return { end: cursor + 1, rawEnd: cursor + 1 };
      if (current === 0x5c) {
        cursor += 1;
        if (cursor >= bytes.length) invalid('json', 'scene JSON has an unfinished escape');
        const escaped = bytes[cursor];
        if (escaped === 0x75) {
          if (cursor + 4 >= bytes.length) invalid('json', 'scene JSON has an unfinished Unicode escape');
          for (let digit = cursor + 1; digit <= cursor + 4; digit += 1) {
            if (!/[0-9a-f]/i.test(String.fromCharCode(bytes[digit]))) invalid('json', 'scene JSON has an invalid Unicode escape');
          }
          stringBytes += 4;
          cursor += 4;
        } else if (![0x22, 0x5c, 0x2f, 0x62, 0x66, 0x6e, 0x72, 0x74].includes(escaped)) {
          invalid('json', 'scene JSON has an invalid escape');
        } else {
          stringBytes += 1;
        }
      } else {
        if (current < 0x20) invalid('json', 'scene JSON contains a control character in text');
        stringBytes += 1;
      }
      if (stringBytes > MAX_SCENE_TEXT_BYTES) resource('text', 'scene string exceeds the bounded text contract');
      cursor += 1;
    }
    invalid('json', 'scene JSON contains an unterminated string');
  };
  const close = (kind) => {
    const frame = stack.pop();
    if (!frame || frame.kind !== kind || (frame.state !== 'comma' && frame.state !== (kind === 'object' ? 'key' : 'value'))) invalid('json', 'scene JSON has an invalid closing delimiter');
  };
  while (index < bytes.length) {
    if ((index & 0x3ff) === 0) checkCancelled(isCancelled);
    const byte = bytes[index];
    if (byte <= 0x20) invalid('canonical-json', 'scene JSON contains non-canonical whitespace');
    const frame = stack[stack.length - 1];
    if (byte === 0x7b) {
      consumeValue();
      if (stack.length >= 64) resource('json-depth', 'scene JSON nesting exceeds 64 levels');
      const parent = frame?.kind === 'object' ? frame.key : undefined;
      stack.push({ kind: 'object', state: 'key', keys: new Set(), key: undefined, field: parent });
      index += 1; scanned += 1; continue;
    }
    if (byte === 0x5b) {
      consumeValue();
      if (stack.length >= 64) resource('json-depth', 'scene JSON nesting exceeds 64 levels');
      const parent = frame?.kind === 'object' ? frame.key : undefined;
      stack.push({ kind: 'array', state: 'value', count: 0, field: parent });
      index += 1; scanned += 1; continue;
    }
    if (byte === 0x7d) { close('object'); index += 1; scanned += 1; continue; }
    if (byte === 0x5d) { close('array'); index += 1; scanned += 1; continue; }
    if (byte === 0x2c) {
      if (!frame || frame.state !== 'comma') invalid('json', 'scene JSON has an unexpected comma');
      frame.state = frame.kind === 'object' ? 'key' : 'value';
      index += 1; scanned += 1; continue;
    }
    if (byte === 0x3a) {
      if (!frame || frame.kind !== 'object' || frame.state !== 'colon') invalid('json', 'scene JSON has an unexpected colon');
      frame.state = 'value'; index += 1; scanned += 1; continue;
    }
    if (byte === 0x22) {
      const string = scanString(index);
      if (frame?.kind === 'object' && frame.state === 'key') {
        let key;
        try { key = JSON.parse(bytesToText(bytes.slice(index, string.rawEnd))); } catch { invalid('json', 'scene JSON contains an invalid object key'); }
        if (frame.keys.has(key)) invalid('canonical-json', `scene JSON contains duplicate key ${key}`);
        frame.keys.add(key); frame.key = key; frame.state = 'colon';
      } else {
        consumeValue();
      }
      index = string.end; scanned += 1; continue;
    }
    // A primitive token is consumed as one value. JSON.parse performs the
    // precise token validation after the structural/resource checks above.
    if (frame?.kind === 'object' && frame.state !== 'value') invalid('json', 'scene JSON has an unexpected token');
    consumeValue();
    while (index < bytes.length && ![0x2c, 0x5d, 0x7d].includes(bytes[index])) {
      if (bytes[index] <= 0x20) invalid('canonical-json', 'scene JSON contains non-canonical whitespace');
      index += 1;
    }
    scanned += 1;
  }
  if (stack.length !== 0) invalid('json', 'scene JSON has unbalanced delimiters');
  if (!rootSeen) invalid('json', 'scene JSON is empty');
  checkCancelled(isCancelled);
  return scanned;
}

function validateConfiguration(value) {
  object(value, 'configuration', ['method', 'support_radius', 'minimum_locations', 'maximum_neighbors', 'extrapolation']);
  const method = value.method;
  if (typeof method === 'string') {
    if (!['PointValue', 'Nearest'].includes(method)) invalid('value', 'unsupported spatial method');
  } else {
    if (method === null || typeof method !== 'object' || Array.isArray(method)) invalid('type', 'configuration.method must be a tagged object');
    const methodKeys = Object.keys(method);
    if (methodKeys.length !== 1 || !['Idw'].includes(methodKeys[0])) invalid('value', 'unsupported spatial method');
  }
  if (typeof method === 'object' && method.Idw !== undefined) {
    object(method.Idw, 'configuration.method.Idw', ['power']);
    finite(method.Idw.power, 'configuration.method.Idw.power');
    if (method.Idw.power <= 0 || method.Idw.power > 64) invalid('value', 'IDW power is outside its bound');
  }
  finite(value.support_radius, 'configuration.support_radius');
  if (value.support_radius <= 0) invalid('value', 'support radius must be positive');
  integer(value.minimum_locations, 'configuration.minimum_locations', 1, 64);
  integer(value.maximum_neighbors, 'configuration.maximum_neighbors', value.minimum_locations, 64);
  if (value.extrapolation !== 'Disabled') {
    if (value.extrapolation === null || typeof value.extrapolation !== 'object' || Object.keys(value.extrapolation).length !== 1 || !('WithinRadius' in value.extrapolation)) invalid('value', 'unsupported extrapolation policy');
    finite(value.extrapolation.WithinRadius, 'configuration.extrapolation.WithinRadius');
    if (value.extrapolation.WithinRadius < value.support_radius) invalid('value', 'extrapolation radius is below support radius');
  }
  return value;
}

function methodKey(method) {
  return typeof method === 'string' ? method : Object.keys(method)[0];
}

function validateMetricDefinition(bytes, scene) {
  preflightJson(bytes);
  let definition;
  try { definition = JSON.parse(bytesToText(bytes)); } catch { invalid('metric-definition', 'metric definition bytes are not canonical JSON'); }
  object(definition, 'metric_definition', ['schema', 'id', 'version', 'semantic_description', 'unit', 'valid_range', 'evidence_requirements', 'aggregation', 'spatial_method', 'selection', 'uncertainty', 'unknown_compatibility', 'compatibility', 'visualization', 'compliance']);
  if (definition.schema !== 'kyberia.metric-definition/1') invalid('metric-definition', 'unsupported metric definition schema');
  text(definition.id, 'metric_definition.id');
  integer(definition.version, 'metric_definition.version', 1, 65535);
  if (`${definition.id}/${definition.version}` !== scene.identity.metric_version || definition.id !== scene.identity.metric_id) invalid('identity', 'metric definition identity differs from scene identity');
  if (!['wifi.rssi', 'wifi.rssi.nearest', 'wifi.rssi.idw'].includes(definition.id)) invalid('metric-definition', 'metric is not an admitted observed RSSI definition');
  const descriptions = { 'wifi.rssi': 'Received Wi-Fi signal power at the observation point', 'wifi.rssi.nearest': 'Nearest supported observed Wi-Fi signal power', 'wifi.rssi.idw': 'Inverse-distance weighted observed Wi-Fi signal power' };
  text(definition.semantic_description, 'metric_definition.semantic_description');
  if (definition.semantic_description !== descriptions[definition.id]) invalid('metric-definition', 'metric semantic description is not canonical');
  if (definition.unit !== 'dbm') invalid('metric-definition', 'scene metric unit is not dBm');
  const spatialMethods = { 'wifi.rssi': 'point_value', 'wifi.rssi.nearest': 'nearest', 'wifi.rssi.idw': 'inverse_distance_weighted' };
  if (definition.spatial_method !== spatialMethods[definition.id]) invalid('metric-definition', 'metric spatial method is inconsistent with its identity');
  if ((definition.spatial_method === 'point_value' && methodKey(scene.configuration.method) !== 'PointValue') || (definition.spatial_method === 'nearest' && methodKey(scene.configuration.method) !== 'Nearest') || (definition.spatial_method === 'inverse_distance_weighted' && methodKey(scene.configuration.method) !== 'Idw')) invalid('identity', 'metric spatial method differs from tile configuration');
  object(definition.valid_range, 'metric_definition.valid_range', ['kind', 'minimum', 'maximum']);
  if (definition.valid_range.kind !== 'numeric' || definition.valid_range.minimum !== -200 || definition.valid_range.maximum !== 100) invalid('metric-definition', 'observed RSSI range is not canonical');
  object(definition.evidence_requirements, 'metric_definition.evidence_requirements', ['evidence', 'capabilities']);
  if (!Array.isArray(definition.evidence_requirements.evidence) || definition.evidence_requirements.evidence.length !== 1 || definition.evidence_requirements.evidence[0] !== 'observed') invalid('metric-definition', 'metric does not require observed evidence');
  if (!Array.isArray(definition.evidence_requirements.capabilities) || definition.evidence_requirements.capabilities.length !== 1) invalid('metric-definition', 'metric capability requirements are not canonical');
  const capability = definition.evidence_requirements.capabilities[0];
  object(capability, 'metric_definition.evidence_requirements.capabilities[0]', ['kind', 'capabilities']);
  if (capability.kind !== 'any_of' || JSON.stringify(capability.capabilities) !== JSON.stringify(['nearby_scan', 'monitor_frames'])) invalid('metric-definition', 'metric capability requirements are not canonical');
  object(definition.aggregation, 'metric_definition.aggregation', ['kind', 'selection']);
  if (definition.aggregation.kind !== 'signal_dbm') invalid('metric-definition', 'metric aggregation is not signal dBm');
  object(definition.aggregation.selection, 'metric_definition.aggregation.selection', ['algorithm_version', 'method']);
  if (definition.aggregation.selection.algorithm_version !== scene.identity.signal_aggregation.algorithm_version || JSON.stringify(definition.aggregation.selection.method) !== JSON.stringify(scene.identity.signal_aggregation.method)) invalid('identity', 'metric aggregation differs from scene provenance');
  object(definition.selection, 'metric_definition.selection', ['filters', 'grouping']);
  if (JSON.stringify(definition.selection.filters) !== JSON.stringify(['band', 'channel']) || JSON.stringify(definition.selection.grouping) !== JSON.stringify(['access_point'])) invalid('metric-definition', 'metric selection is not canonical');
  object(definition.compatibility, 'metric_definition.compatibility', ['kind']);
  object(definition.visualization, 'metric_definition.visualization', ['palette', 'display_precision']);
  if (definition.visualization.palette !== 'sequential' || definition.visualization.display_precision !== 1) invalid('metric-definition', 'metric visualization is not canonical');
  if (definition.uncertainty !== 'not_reported' || definition.compatibility.kind !== 'exact_evidence_contract' || definition.unknown_compatibility !== 'propagate_reason' || definition.compliance !== 'higher_is_better') invalid('metric-definition', 'metric unknown semantics are not canonical');
  return definition;
}

function validateSceneShape(scene, isCancelled) {
  object(scene, 'scene', ['schema', 'identity', 'metric_artifact', 'metric_definition_bytes', 'evidence_plane', 'configuration', 'samples', 'grid', 'location_groups', 'cells']);
  if (scene.schema !== SCENE_WIRE_SCHEMA) {
    if (typeof scene.schema === 'string') unsupported('schema-version', `scene schema ${scene.schema} is unsupported`);
    invalid('schema-version', 'scene schema is not text');
  }
  text(scene.evidence_plane, 'evidence_plane');
  if (!['Measured', 'Synthetic'].includes(scene.evidence_plane)) invalid('value', 'unsupported evidence plane');
  object(scene.identity, 'identity', ['metric_id', 'metric_version', 'metric_revision', 'metric_definition_hash', 'signal_aggregation', 'source_artifact', 'spatial_schema', 'algorithm_version']);
  text(scene.identity.metric_id, 'identity.metric_id');
  text(scene.identity.metric_version, 'identity.metric_version');
  integer(scene.identity.metric_revision, 'identity.metric_revision', 1, 65535);
  hash(scene.identity.metric_definition_hash, 'identity.metric_definition_hash');
  text(scene.identity.spatial_schema, 'identity.spatial_schema');
  text(scene.identity.algorithm_version, 'identity.algorithm_version');
  sourceArtifact(scene.identity.source_artifact, 'identity.source_artifact');
  object(scene.identity.signal_aggregation, 'identity.signal_aggregation', ['algorithm_version', 'method']);
  text(scene.identity.signal_aggregation.algorithm_version, 'identity.signal_aggregation.algorithm_version');
  object(scene.identity.signal_aggregation.method, 'identity.signal_aggregation.method', ['method']);
  text(scene.identity.signal_aggregation.method.method, 'identity.signal_aggregation.method.method');
  if (!AGGREGATION_METHODS.has(scene.identity.signal_aggregation.method.method)) invalid('value', 'unsupported aggregation method');
  artifact(scene.metric_artifact, 'metric_artifact');
  if (scene.metric_artifact.sha256 !== scene.identity.metric_definition_hash) invalid('identity', 'metric artifact and identity hashes differ');
  if (scene.metric_artifact.version !== scene.identity.metric_version) invalid('identity', 'metric artifact version differs from scene identity');
  if (scene.metric_artifact.media_type !== 'application/kyberia-metric-definition+json') invalid('identity', 'metric artifact media type is not canonical');
  if (!Array.isArray(scene.metric_definition_bytes)) invalid('type', 'metric_definition_bytes must be an array');
  if (scene.metric_definition_bytes.length > MAX_METRIC_DEFINITION_BYTES) resource('metric-definition', 'metric definition exceeds its bound');
  scene.metric_definition_bytes.forEach((byte, index) => integer(byte, `metric_definition_bytes[${index}]`, 0, 255));
  if (exactInteger(scene.metric_artifact.byte_length, 'metric_artifact.byte_length') !== scene.metric_definition_bytes.length) invalid('identity', 'metric artifact length differs from definition bytes');
  validateConfiguration(scene.configuration);

  object(scene.grid, 'grid', ['floor_id', 'frame_id', 'origin', 'resolution', 'column_offset', 'row_offset', 'width', 'height']);
  id(scene.grid.floor_id, 'grid.floor_id');
  id(scene.grid.frame_id, 'grid.frame_id');
  point(scene.grid.origin, 'grid.origin');
  finite(scene.grid.resolution, 'grid.resolution');
  if (scene.grid.resolution <= 0) invalid('value', 'grid resolution must be positive');
  integer(scene.grid.column_offset, 'grid.column_offset', 0, 0xffffffff);
  integer(scene.grid.row_offset, 'grid.row_offset', 0, 0xffffffff);
  integer(scene.grid.width, 'grid.width', 1, MAX_SCENE_CELLS);
  integer(scene.grid.height, 'grid.height', 1, MAX_SCENE_CELLS);
  if (scene.grid.width * scene.grid.height > MAX_SCENE_CELLS || scene.grid.column_offset + scene.grid.width > 0xffffffff || scene.grid.row_offset + scene.grid.height > 0xffffffff) resource('grid', 'grid dimensions exceed their bound');

  if (!Array.isArray(scene.samples)) invalid('type', 'samples must be an array');
  if (scene.samples.length > MAX_SCENE_SAMPLES) resource('samples', 'samples exceed their bound');
  const samples = new Map();
  scene.samples.forEach((sample, index) => {
    checkCancelled(isCancelled);
    object(sample, `samples[${index}]`, ['observation_id', 'floor_id', 'frame_id', 'position', 'value', 'position_covariance']);
    id(sample.observation_id, `samples[${index}].observation_id`);
    id(sample.floor_id, `samples[${index}].floor_id`);
    id(sample.frame_id, `samples[${index}].frame_id`);
    if (sample.floor_id !== scene.grid.floor_id || sample.frame_id !== scene.grid.frame_id) invalid('geometry', 'sample frame does not match scene grid');
    point(sample.position, `samples[${index}].position`);
    evidence(sample.value, `samples[${index}].value`, 'dbm');
    evidence(sample.position_covariance, `samples[${index}].position_covariance`, 'covariance');
    if (samples.has(sample.observation_id)) invalid('identity', 'duplicate sample observation ID');
    samples.set(sample.observation_id, sample);
  });

  if (!Array.isArray(scene.location_groups)) invalid('type', 'location_groups must be an array');
  if (scene.location_groups.length > MAX_SCENE_GROUPS) resource('groups', 'location groups exceed their bound');
  const grouped = new Set();
  scene.location_groups.forEach((group, index) => {
    checkCancelled(isCancelled);
    object(group, `location_groups[${index}]`, ['position', 'observation_ids', 'signal_aggregate']);
    point(group.position, `location_groups[${index}].position`);
    if (!Array.isArray(group.observation_ids) || group.observation_ids.length === 0 || group.observation_ids.length > MAX_SCENE_SAMPLES) resource('group-observations', 'location group observation IDs exceed their bound');
    object(group.signal_aggregate, `location_groups[${index}].signal_aggregate`, ['algorithm_version', 'method', 'estimate', 'percentile_interval', 'sample_count', 'observation_order']);
    text(group.signal_aggregate.algorithm_version, `location_groups[${index}].signal_aggregate.algorithm_version`);
    object(group.signal_aggregate.method, `location_groups[${index}].signal_aggregate.method`, ['method']);
    text(group.signal_aggregate.method.method, `location_groups[${index}].signal_aggregate.method.method`);
    if (!AGGREGATION_METHODS.has(group.signal_aggregate.method.method)) invalid('value', 'unsupported group aggregation method');
    evidence(group.signal_aggregate.estimate, `location_groups[${index}].signal_aggregate.estimate`, 'dbm');
    evidence(group.signal_aggregate.percentile_interval, `location_groups[${index}].signal_aggregate.percentile_interval`, 'db');
    integer(group.signal_aggregate.sample_count, `location_groups[${index}].signal_aggregate.sample_count`, 1, MAX_SCENE_SAMPLES);
    if (!Array.isArray(group.signal_aggregate.observation_order) || group.signal_aggregate.observation_order.length > MAX_SCENE_SAMPLES) resource('group-observations', 'observation order exceeds its bound');
    if (group.signal_aggregate.sample_count !== group.observation_ids.length || group.signal_aggregate.observation_order.length !== group.observation_ids.length) invalid('provenance', 'aggregate sample count/order differs from group IDs');
    group.observation_ids.forEach((observationId, idIndex) => {
      id(observationId, `location_groups[${index}].observation_ids[${idIndex}]`);
      if (group.signal_aggregate.observation_order[idIndex] !== observationId) invalid('provenance', 'aggregate order differs from group IDs');
      if (grouped.has(observationId) || !samples.has(observationId)) invalid('provenance', 'group observation ID is duplicated or missing');
      const sample = samples.get(observationId);
      if (sample.value.state !== 'known' || sample.position.x !== group.position.x || sample.position.y !== group.position.y) invalid('provenance', 'group does not match its sample evidence');
      grouped.add(observationId);
    });
  });
  for (const sample of samples.values()) if (sample.value.state === 'known' && !grouped.has(sample.observation_id)) invalid('provenance', 'known sample is absent from location groups');

  if (!Array.isArray(scene.cells)) invalid('type', 'cells must be an array');
  const cellCount = scene.grid.width * scene.grid.height;
  if (scene.cells.length !== cellCount) invalid('geometry', 'cell count differs from grid dimensions');
  let contributionCount = 0;
  scene.cells.forEach((cell, index) => {
    checkCancelled(isCancelled);
    object(cell, `cells[${index}]`, ['value', 'class', 'support_locations', 'support_observations', 'nearest_distance', 'uncertainty_db', 'contributors']);
    text(cell.class, `cells[${index}].class`);
    if (!SCENE_CLASSES.has(cell.class)) invalid('value', 'unsupported cell class');
    evidence(cell.value, `cells[${index}].value`, 'dbm');
    evidence(cell.nearest_distance, `cells[${index}].nearest_distance`, 'meters');
    evidence(cell.uncertainty_db, `cells[${index}].uncertainty_db`, 'db');
    exactInteger(cell.support_locations, `cells[${index}].support_locations`, 0, scene.location_groups.length);
    exactInteger(cell.support_observations, `cells[${index}].support_observations`, 0, MAX_SCENE_SAMPLES);
    if (!Array.isArray(cell.contributors)) invalid('type', `cells[${index}].contributors must be an array`);
    if (cell.contributors.length > 64) resource('contributors', 'cell contributors exceed their bound');
    contributionCount += cell.contributors.length;
    if (contributionCount > MAX_SCENE_CONTRIBUTIONS) resource('contributors', 'scene contributors exceed their bound');
    const contributorIds = new Set();
    let weightSum = 0;
    cell.contributors.forEach((contributor, contributorIndex) => {
      object(contributor, `cells[${index}].contributors[${contributorIndex}]`, ['location_group', 'weight']);
      const groupIndex = exactInteger(contributor.location_group, `cells[${index}].contributors[${contributorIndex}].location_group`, 0, scene.location_groups.length - 1);
      if (contributorIds.has(groupIndex)) invalid('provenance', 'cell contributor group is duplicated');
      contributorIds.add(groupIndex);
      finite(contributor.weight, `cells[${index}].contributors[${contributorIndex}].weight`);
      if (contributor.weight <= 0 || contributor.weight > 1) invalid('value', 'cell contributor weight is outside (0,1]');
      weightSum += contributor.weight;
    });
    if (cell.contributors.length > 0 && Math.abs(weightSum - 1) > 1e-9) invalid('provenance', 'cell contributor weights are not normalized');
    if (cell.value.state === 'unknown' && cell.class !== 'unknown') invalid('provenance', 'unknown value has a known cell class');
    if (cell.value.state === 'known' && cell.class === 'unknown') invalid('provenance', 'known value has an unknown cell class');
    if (cell.class === 'unknown' && cell.contributors.length > 0) invalid('provenance', 'unknown cell has contributors');
    if (cell.class !== 'unknown' && cell.contributors.length === 0) invalid('provenance', 'known cell has no contributors');
    if (cell.class === 'extrapolated' && scene.configuration.extrapolation === 'Disabled') invalid('provenance', 'extrapolated cell violates disabled extrapolation policy');
  });
  return { samples, grouped, cellCount };
}

function expectedCell(scene, index, groups) {
  const column = index % scene.grid.width;
  const row = Math.floor(index / scene.grid.width);
  const x = scene.grid.origin.x + (scene.grid.column_offset + column + 0.5) * scene.grid.resolution;
  const y = scene.grid.origin.y + (scene.grid.row_offset + row + 0.5) * scene.grid.resolution;
  const distances = groups.map((group, groupIndex) => ({ groupIndex, distance: Math.hypot(x - group.position.x, y - group.position.y) }));
  distances.sort((a, b) => a.distance - b.distance || a.groupIndex - b.groupIndex);
  const nearest = distances[0] || null;
  if (!nearest) return { x, y, class: 'unknown', value: null, support: [], nearest: null, contributors: [] };
  const support = distances.filter(({ distance }) => distance <= scene.configuration.support_radius);
  const selectedMethod = methodKey(scene.configuration.method);
  const exact = distances.find(({ distance }) => distance === 0);
  if (exact) return { x, y, class: 'observed', value: groups[exact.groupIndex].signal_aggregate.estimate.detail, support, nearest, contributors: [{ groupIndex: exact.groupIndex, weight: 1 }] };
  if (selectedMethod === 'PointValue') return { x, y, class: 'unknown', value: null, support, nearest, contributors: [] };
  const extrapolationRadius = scene.configuration.extrapolation === 'Disabled' ? scene.configuration.support_radius : scene.configuration.extrapolation.WithinRadius;
  let candidates = distances.filter(({ distance }) => Number.isFinite(distance) && distance <= extrapolationRadius);
  if (support.length >= scene.configuration.minimum_locations) candidates = candidates.filter(({ distance }) => distance <= scene.configuration.support_radius);
  candidates = candidates.slice(0, scene.configuration.maximum_neighbors);
  if (support.length < scene.configuration.minimum_locations && scene.configuration.extrapolation === 'Disabled') return { x, y, class: 'unknown', value: null, support, nearest, contributors: [] };
  if (candidates.length < scene.configuration.minimum_locations) return { x, y, class: 'unknown', value: null, support, nearest, contributors: [] };
  const selected = selectedMethod === 'Nearest' ? candidates.slice(0, 1) : candidates;
  const weights = selectedMethod === 'Nearest' ? selected.map(() => 1) : selected.map(({ distance }) => (candidates[0].distance / distance) ** scene.configuration.method.Idw.power);
  const denominator = weights.reduce((sum, weight) => sum + weight, 0);
  const contributors = selected.map(({ groupIndex }, selectedIndex) => ({ groupIndex, weight: weights[selectedIndex] / denominator }));
  const value = contributors.reduce((sum, contributor) => sum + groups[contributor.groupIndex].signal_aggregate.estimate.detail * contributor.weight, 0);
  return { x, y, class: support.length >= scene.configuration.minimum_locations ? 'interpolated' : 'extrapolated', value, support, nearest, contributors };
}

function aggregateSignal(values, method) {
  const sorted = [...values].sort((left, right) => left - right);
  if (method === 'median_dbm') {
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
  }
  if (method === 'linear_power_mean') {
    const maximum = Math.max(...values);
    const scaled = values.reduce((sum, value) => sum + (10 ** ((value - maximum) / 10)), 0) / values.length;
    return maximum + 10 * Math.log10(scaled);
  }
  unsupported('aggregation', `browser replay does not support ${method} aggregation yet`);
}

function validateAggregates(scene, samples, isCancelled) {
  const identityMethod = scene.identity.signal_aggregation.method.method;
  const identityAlgorithm = scene.identity.signal_aggregation.algorithm_version;
  scene.location_groups.forEach((group, index) => {
    checkCancelled(isCancelled);
    const aggregate = group.signal_aggregate;
    if (aggregate.algorithm_version !== identityAlgorithm || aggregate.method.method !== identityMethod) invalid('identity', `location_groups[${index}] aggregation provenance differs from scene identity`);
    const values = group.observation_ids.map((observationId) => samples.get(observationId).value.detail);
    const expected = aggregateSignal(values, aggregate.method.method);
    if (aggregate.estimate.state !== 'known' || Math.abs(aggregate.estimate.detail - expected) > 1e-9) invalid('numerical-replay', `location_groups[${index}] aggregate differs from the canonical signal replay`);
    if (aggregate.percentile_interval.state !== 'unknown' || aggregate.percentile_interval.detail !== 'not_applicable') invalid('numerical-replay', `location_groups[${index}] percentile interval is not canonical for ${aggregate.method.method}`);
  });
}

function validateNumericalReplay(scene, definition, isCancelled) {
  const groups = scene.location_groups;
  scene.cells.forEach((cell, index) => {
    if ((index & 0x3f) === 0) checkCancelled(isCancelled);
    const expected = expectedCell(scene, index, groups);
    if (cell.class !== expected.class) invalid('numerical-replay', `cells[${index}] class differs from the canonical model`);
    if (expected.value === null) {
      const expectedReason = groups.length === 0 ? 'not_measured' : 'outside_evidence_support';
      if (cell.value.state !== 'unknown' || cell.value.detail !== expectedReason) invalid('numerical-replay', `cells[${index}] unknown state differs from the canonical model`);
    } else {
      if (cell.value.state !== 'known' || Math.abs(cell.value.detail - expected.value) > 1e-6) invalid('numerical-replay', `cells[${index}] value differs from the canonical model`);
    }
    const supportLocations = exactInteger(cell.support_locations, `cells[${index}].support_locations`);
    const supportObservations = exactInteger(cell.support_observations, `cells[${index}].support_observations`);
    const expectedObservations = expected.support.reduce((sum, group) => sum + groups[group.groupIndex].observation_ids.length, 0);
    if (supportLocations !== expected.support.length || supportObservations !== expectedObservations) invalid('numerical-replay', `cells[${index}] support differs from the canonical model`);
    if (expected.nearest === null) {
      if (cell.nearest_distance.state !== 'unknown' || cell.nearest_distance.detail !== 'not_measured') invalid('numerical-replay', `cells[${index}] nearest distance differs from the canonical model`);
    } else if (cell.nearest_distance.state !== 'known' || Math.abs(cell.nearest_distance.detail - expected.nearest.distance) > 1e-9) invalid('numerical-replay', `cells[${index}] nearest distance differs from the canonical model`);
    if (cell.contributors.length !== expected.contributors.length || cell.contributors.some((contributor, contributorIndex) => exactInteger(contributor.location_group, 'contributor') !== expected.contributors[contributorIndex].groupIndex || Math.abs(contributor.weight - expected.contributors[contributorIndex].weight) > 1e-6)) invalid('numerical-replay', `cells[${index}] contributors differ from the canonical model`);
    if (definition.uncertainty === 'not_reported' && (cell.uncertainty_db.state !== 'unknown' || cell.uncertainty_db.detail !== 'not_measured')) invalid('numerical-replay', `cells[${index}] reports uncertainty for a metric whose contract does not provide it`);
  });
}

function bytesToText(bytes) {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    invalid('encoding', 'scene bytes are not valid UTF-8');
  }
}

function bytesToHex(bytes) { return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join(''); }

function sceneInputBytes(input) {
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  invalid('type', 'scene input must be bytes');
}

function wasmAdmissionError(status, detail) {
  if (status === 3) return new SceneLoadError('cancelled', 'cancelled', 'scene WASM admission cancelled');
  if (status === 4) return new SceneLoadError('unsupported', 'schema-version', 'scene WASM validator does not support this scene version');
  if (status === 5) return new SceneLoadError('resource-limit', 'wasm-resource', 'scene WASM validator rejected the resource bounds');
  return new SceneLoadError('invalid', status === 6 ? 'canonical-bytes' : 'wasm-admission', detail || 'scene WASM validator rejected the bytes');
}

/**
 * Admit bytes through the Rust rendering-scene contract in a worker. A fresh
 * worker per request makes cancellation and a malformed-input trap explicit;
 * the UI never falls back to the JavaScript mirror when this boundary is
 * unavailable or rejects the bytes.
 */
export async function validateCanonicalSceneWithWasm(input, { isCancelled, deadlineMs = MAX_WASM_ADMISSION_MS } = {}) {
  const bytes = sceneInputBytes(input);
  if (bytes.byteLength > MAX_SCENE_BYTES) resource('scene-bytes', 'scene exceeds the 64 MiB import bound');
  checkCancelled(isCancelled);
  if (!Number.isInteger(deadlineMs) || deadlineMs <= 0 || deadlineMs > MAX_WASM_ADMISSION_MS) invalid('wasm-timeout', `WASM admission deadline must be an integer between 1 and ${MAX_WASM_ADMISSION_MS} ms`);
  if (typeof globalThis.Worker !== 'function') unsupported('wasm-runtime', 'browser Web Workers are unavailable for canonical scene admission');
  const worker = new Worker(new URL('./scene-validator-worker.js', import.meta.url), { type: 'module' });
  const transferred = bytes.slice();
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setInterval(() => {
      if (isCancelled?.()) finish(new SceneLoadError('cancelled', 'cancelled', 'scene WASM admission cancelled'));
    }, 20);
    const deadline = setTimeout(() => finish(new SceneLoadError('resource-limit', 'wasm-timeout', `scene WASM admission exceeded ${deadlineMs} ms`)), deadlineMs);
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearInterval(timer);
      clearTimeout(deadline);
      worker.onmessage = null;
      worker.onerror = null;
      worker.terminate();
      if (error) reject(error); else resolve(value);
    };
    worker.onmessage = ({ data }) => {
      if (data?.id !== 1) return;
      if (typeof data.error === 'string') {
        finish(data.code === 'timeout'
          ? new SceneLoadError('resource-limit', 'wasm-timeout', data.error)
          : new SceneLoadError('unsupported', 'wasm-runtime', data.error));
      } else if (data.status === 0) {
        finish(null, { status: 'accepted' });
      } else {
        finish(wasmAdmissionError(data.status, 'scene WASM validator rejected the canonical bytes'));
      }
    };
    worker.onerror = (event) => finish(new SceneLoadError('unsupported', 'wasm-runtime', event.message || 'scene WASM validator failed'));
    if (isCancelled?.()) {
      finish(new SceneLoadError('cancelled', 'cancelled', 'scene WASM admission cancelled'));
      return;
    }
    try {
      worker.postMessage({ id: 1, bytes: transferred.buffer, deadlineMs }, [transferred.buffer]);
    } catch (error) {
      finish(new SceneLoadError('unsupported', 'wasm-runtime', error?.message || String(error)));
    }
  });
}

async function sha256Hex(bytes, isCancelled) {
  checkCancelled(isCancelled);
  if (!globalThis.crypto?.subtle) unsupported('crypto', 'Web Crypto SHA-256 is unavailable');
  const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes);
  checkCancelled(isCancelled);
  return bytesToHex(new Uint8Array(digest));
}

function toLayer(scene) {
  const values = new Float32Array(scene.cells.length);
  const mask = new Uint8Array(scene.cells.length);
  scene.cells.forEach((cell, index) => {
    if (cell.value.state === 'known') {
      values[index] = cell.value.detail;
      mask[index] = { observed: 1, interpolated: 2, extrapolated: 3 }[cell.class];
    } else {
      values[index] = Number.NaN;
      mask[index] = 0;
    }
  });
  const knownCell = scene.cells.findIndex((cell) => cell.class === 'observed');
  const unknownCell = scene.cells.findIndex((cell) => cell.class === 'unknown');
  return {
    id: `${scene.identity.metric_id}@${scene.identity.metric_version}`,
    width: scene.grid.width,
    height: scene.grid.height,
    values,
    mask,
    unit: 'dBm',
    worldBounds: [scene.grid.origin.x + scene.grid.column_offset * scene.grid.resolution, scene.grid.origin.y + scene.grid.row_offset * scene.grid.resolution, scene.grid.origin.x + (scene.grid.column_offset + scene.grid.width) * scene.grid.resolution, scene.grid.origin.y + (scene.grid.row_offset + scene.grid.height) * scene.grid.resolution],
    worldUnit: 'metres',
    scene,
    probes: { knownCell: knownCell < 0 ? 0 : knownCell, unknownCell: unknownCell < 0 ? 0 : unknownCell },
  };
}

/**
 * Parse and validate a canonical scene. The returned `layer` is derived only
 * after schema, bounds, hash, provenance, and numerical replay checks pass.
 */
async function loadCanonicalSceneAfterAdmission(input, { isCancelled } = {}) {
  let bytes = sceneInputBytes(input);
  if (bytes.byteLength > MAX_SCENE_BYTES) resource('scene-bytes', 'scene exceeds the 64 MiB import bound');
  bytes = bytes.slice();
  checkCancelled(isCancelled);
  preflightJson(bytes, isCancelled);
  const textValue = bytesToText(bytes);
  checkCancelled(isCancelled);
  let scene;
  try { scene = JSON.parse(textValue); } catch { invalid('json', 'scene JSON is malformed'); }
  const shape = validateSceneShape(scene, isCancelled);
  const metricBytes = Uint8Array.from(scene.metric_definition_bytes);
  const metricHash = await sha256Hex(metricBytes, isCancelled);
  if (metricHash !== scene.metric_artifact.sha256 || metricHash !== scene.identity.metric_definition_hash) invalid('hash', 'metric definition hash does not match its bytes');
  const definition = validateMetricDefinition(metricBytes, scene);
  validateAggregates(scene, shape.samples, isCancelled);
  validateNumericalReplay(scene, definition, isCancelled);
  checkCancelled(isCancelled);
  const sourceHash = await sha256Hex(bytes, isCancelled);
  const layer = toLayer(scene);
  return Object.freeze({
    contract: SCENE_CONTRACT_ID,
    wireSchema: scene.schema,
    bytes,
    byteLength: bytes.byteLength,
    sha256: sourceHash,
    scene,
    layer,
    grid: scene.grid,
    identity: scene.identity,
    metricArtifact: scene.metric_artifact,
    evidencePlane: scene.evidence_plane,
    cellGeometry: { floorId: scene.grid.floor_id, frameId: scene.grid.frame_id, origin: scene.grid.origin, resolution: scene.grid.resolution, columnOffset: scene.grid.column_offset, rowOffset: scene.grid.row_offset, width: scene.grid.width, height: scene.grid.height },
    counts: { samples: scene.samples.length, groups: scene.location_groups.length, cells: scene.cells.length, knownCells: scene.cells.filter((cell) => cell.value.state === 'known').length, unknownCells: scene.cells.filter((cell) => cell.value.state === 'unknown').length },
  });
}

/**
 * Load a scene only after the required Rust canonical admission boundary has
 * accepted its exact bytes. The validator is deliberately not injectable:
 * callers that need the JavaScript mirror for diagnostics must opt into the
 * separately named `loadCanonicalSceneMirror` function below.
 */
export async function loadCanonicalScene(input, { isCancelled } = {}) {
  let bytes = sceneInputBytes(input);
  if (bytes.byteLength > MAX_SCENE_BYTES) resource('scene-bytes', 'scene exceeds the 64 MiB import bound');
  bytes = bytes.slice();
  checkCancelled(isCancelled);
  await validateCanonicalSceneWithWasm(bytes, { isCancelled });
  checkCancelled(isCancelled);
  return loadCanonicalSceneAfterAdmission(bytes, { isCancelled });
}

/**
 * Diagnostic-only JavaScript replay path. It is useful for numerical replay
 * tests and for comparing browser behavior, but it is not a canonical import
 * boundary and must not be used by production file loading.
 */
export async function loadCanonicalSceneMirror(input, { isCancelled } = {}) {
  return loadCanonicalSceneAfterAdmission(input, { isCancelled });
}

export function sceneCellCenter(scene, index) {
  integer(index, 'scene cell index', 0, scene.cells.length - 1);
  const column = index % scene.grid.width;
  const row = Math.floor(index / scene.grid.width);
  return { x: scene.grid.origin.x + (scene.grid.column_offset + column + 0.5) * scene.grid.resolution, y: scene.grid.origin.y + (scene.grid.row_offset + row + 0.5) * scene.grid.resolution, column, row };
}

export function sceneStatus(error) {
  if (error instanceof SceneLoadError) return { state: error.state === 'resource-limit' ? 'error' : error.state, detail: `${error.code}: ${error.message}` };
  return { state: 'error', detail: `load: ${error?.message || String(error)}` };
}
