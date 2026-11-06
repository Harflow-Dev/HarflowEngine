'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createWriter } = require('./client_reason_writer.cjs');

const NONCE = '0123456789abcdef0123456789abcdef';

function prepare(file, nonce = NONCE) {
  const fd = fs.openSync(file, 'wx', 0o600);
  fs.closeSync(fd);
  const stamp = fs.statSync(file, { bigint: true });
  process.env.HARFLOW_TLS_OBSERVER_FILE = file;
  process.env.HARFLOW_TLS_OBSERVER_NONCE = nonce;
  process.env.HARFLOW_TLS_OBSERVER_DEV = stamp.dev.toString();
  process.env.HARFLOW_TLS_OBSERVER_INODE = stamp.ino.toString();
}

function environment() {
  const keys = ['HARFLOW_TLS_OBSERVER_FILE', 'HARFLOW_TLS_OBSERVER_NONCE',
    'HARFLOW_TLS_OBSERVER_DEV', 'HARFLOW_TLS_OBSERVER_INODE'];
  return keys.map(key => [key, process.env[key]]);
}

function restoreEnvironment(saved) {
  for (const [key, value] of saved) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
}

function verifiesBoundFile(file) {
  const stat = fs.statSync(file, { bigint: true });
  assert.equal(stat.nlink, 1n);
  assert.equal(stat.mode & 0o777n, 0o600n);
  assert.equal(stat.dev.toString(), process.env.HARFLOW_TLS_OBSERVER_DEV);
  assert.equal(stat.ino.toString(), process.env.HARFLOW_TLS_OBSERVER_INODE);
}

function writesExactMaximumAndRefusesByte513(root) {
  const file = path.join(root, 'maximum');
  prepare(file);
  const descriptor = Object.getOwnPropertyDescriptor(process, 'pid');
  Object.defineProperty(process, 'pid', { configurable: true, value: 4294967295 });
  try {
    const writer = createWriter();
    assert.equal(writer.write(['CR2']), true);
    for (let call = 1; call <= 4; call++) {
      assert.equal(writer.write(['CI2', call]), true);
      assert.equal(writer.write(['CN2', call]), true);
      assert.equal(writer.write(['CE2', call, 'not_yet_valid', 'not_yet_valid']), true);
    }
    assert.equal(writer.write(['CO2']), true);
    assert.equal(writer.write(['CX2', 4, 4, 1, 1]), true);
    const data = fs.readFileSync(file);
    assert.equal(data.length, 512);
    assert.equal(data.toString('ascii').split('\n').length, 16);
    assert.match(data.toString('ascii'), new RegExp(`^CR2\\t${NONCE}\\t4294967295\\n`));
    verifiesBoundFile(file);
    const before = fs.readFileSync(file);
    assert.equal(writer.write(['CF2', 1]), false);
    assert.deepEqual(fs.readFileSync(file), before);
    assert.equal(fs.statSync(file).size, 512);
  } finally {
    Object.defineProperty(process, 'pid', descriptor);
  }
}

function refusesWrongPathsAndPreservesFileSizes(root) {
  const target = path.join(root, 'target');
  fs.writeFileSync(target, 'untouched', { mode: 0o600 });
  const symlink = path.join(root, 'symlink');
  fs.symlinkSync(target, symlink);
  prepareIdentity(symlink);
  assert.equal(createWriter().write(['CR2']), false);
  assert.equal(fs.readFileSync(target, 'utf8'), 'untouched');

  const linked = path.join(root, 'linked');
  const alias = path.join(root, 'alias');
  fs.writeFileSync(linked, '', { mode: 0o600 });
  fs.linkSync(linked, alias);
  prepareIdentity(linked);
  assert.equal(createWriter().write(['CR2']), false);
  assert.equal(fs.statSync(linked).size, 0);

  const wrongMode = path.join(root, 'wrong-mode');
  fs.writeFileSync(wrongMode, '', { mode: 0o644 });
  prepareIdentity(wrongMode);
  assert.equal(createWriter().write(['CR2']), false);
  assert.equal(fs.statSync(wrongMode).size, 0);

  const wrongIdentity = path.join(root, 'wrong-identity');
  fs.writeFileSync(wrongIdentity, '', { mode: 0o600 });
  prepareIdentity(wrongIdentity);
  process.env.HARFLOW_TLS_OBSERVER_INODE = '0';
  assert.equal(createWriter().write(['CR2']), false);
  assert.equal(fs.statSync(wrongIdentity).size, 0);

  const overBudget = path.join(root, 'over-budget');
  fs.writeFileSync(overBudget, 'x'.repeat(500), { mode: 0o600 });
  prepareIdentity(overBudget);
  assert.equal(createWriter().write(['CR2']), false);
  assert.equal(fs.statSync(overBudget).size, 500);
}

function prepareIdentity(file) {
  const stamp = fs.lstatSync(file, { bigint: true });
  process.env.HARFLOW_TLS_OBSERVER_FILE = file;
  process.env.HARFLOW_TLS_OBSERVER_NONCE = NONCE;
  process.env.HARFLOW_TLS_OBSERVER_DEV = stamp.dev.toString();
  process.env.HARFLOW_TLS_OBSERVER_INODE = stamp.ino.toString();
}

function main() {
  const saved = environment();
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'harflow-client-reason-'));
  try {
    writesExactMaximumAndRefusesByte513(root);
    refusesWrongPathsAndPreservesFileSizes(root);
  } finally {
    restoreEnvironment(saved);
    fs.rmSync(root, { recursive: true, force: true });
  }
}

main();
