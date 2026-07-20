/**
 * Stub for `ws`'s optional native addons (`bufferutil`, `utf-8-validate`).
 * ws uses them only as performance hints and guards their absence with
 * try/catch, but the main-process bundle turns the imports into hard
 * ESM imports that fail at load. Aliasing them here keeps the bundle
 * self-contained; ws falls back to its pure-JS paths.
 */
export default {};
