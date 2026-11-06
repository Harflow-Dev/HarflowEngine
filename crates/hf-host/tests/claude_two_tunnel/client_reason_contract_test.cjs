'use strict';
const assert = require('node:assert/strict');
const { classify } = require('./client_reason_classifier.cjs');

function rejectsWithoutReadingProperties() {
  const error = {};
  for (const key of ['message', 'stack', 'name'])
    Object.defineProperty(error, key, { get() { throw Error('poison'); } });
  Object.defineProperty(error, 'code', { get() { throw Error('poison'); } });
  Object.defineProperty(error, 'cause', { get() { throw Error('poison'); } });
  assert.deepEqual(classify(error), { outer: 'accessor', cause: 'accessor' });
  const trap = new Proxy({}, { getOwnPropertyDescriptor() { throw Error('poison'); } });
  assert.deepEqual(classify(trap), { outer: 'trap', cause: 'trap' });
  const coercion = { [Symbol.toPrimitive]() { throw Error('coerced'); } };
  const oversized = Object.defineProperty({}, 'code', { value: 'X'.repeat(65) });
  const unknown = Object.defineProperty({}, 'code', { value: 'unlisted' });
  assert.deepEqual(classify({ code: coercion }), { outer: 'unknown', cause: 'missing' });
  assert.deepEqual(classify(oversized), { outer: 'unknown', cause: 'missing' });
  assert.deepEqual(classify(unknown), { outer: 'unknown', cause: 'missing' });
  assert.deepEqual(classify(null), { outer: 'missing', cause: 'missing' });
}

async function preservesFetchOutcomesAndBoundsReactions() {
  const rows = [];
  let receiver, receivedArgs, nativeCalls = 0, timers = 0;
  const cause = Object.defineProperty({}, 'code', { value: 'UNABLE_TO_VERIFY_LEAF_SIGNATURE' });
  const nativeError = Object.assign(new Error('private text'), { code: 'ECONNRESET', cause });
  let rejectPromise;
  const rejected = new Promise((_, reject) => { rejectPromise = reject; });
  function native(...args) {
    nativeCalls++;
    receiver = this;
    receivedArgs = args;
    if (nativeCalls === 1) throw nativeError;
    if (nativeCalls === 2) return rejected;
    return Promise.resolve(`result-${nativeCalls}`);
  }
  native.preconnect = function preconnect(...args) { return [this, args]; };
  const originalSetTimeout = global.setTimeout;
  const originalThen = Promise.prototype.then;
  let installedReactions = 0;
  Promise.prototype.then = function (...args) {
    installedReactions++;
    return Reflect.apply(originalThen, this, args);
  };
  global.setTimeout = (...args) => { timers++; return originalSetTimeout(...args); };
  try {
    delete require.cache[require.resolve('./client_reason_hook.cjs')];
    const { install } = require('./client_reason_hook.cjs');
    const writer = { write(row) { rows.push(row); return true; } };
    const { proxy, finish } = install(native, writer);
    const thisArg = { sentinel: true }, args = ['/fixed', { method: 'GET' }];
    assert.throws(() => Reflect.apply(proxy, thisArg, args), error => error === nativeError);
    assert.equal(receiver, thisArg);
    assert.deepEqual(receivedArgs, args);
    assert.equal(receivedArgs[0], args[0]);
    assert.equal(receivedArgs[1], args[1]);
    const originalPromise = Reflect.apply(proxy, thisArg, args);
    rejectPromise(nativeError);
    const pending = [3, 4].map(i => Reflect.apply(proxy, thisArg, [String(i)]));
    const beforeOverflow = installedReactions;
    const overflowResult = Reflect.apply(proxy, thisArg, ['overflow']);
    assert(beforeOverflow <= 4, 'at most the first four calls receive rejection reactions');
    assert.equal(installedReactions, beforeOverflow, 'overflow adds no rejection reaction');
    Promise.prototype.then = originalThen;
    const originalOutcome = originalPromise.then(() => 'unexpected', e => e);
    assert.equal(await originalOutcome, nativeError);
    assert.equal(await overflowResult, 'result-5');
    assert.deepEqual(await Promise.all(pending), ['result-3', 'result-4']);
    const preconnectThis = { marker: 1 };
    assert.deepEqual(proxy.preconnect.call(preconnectThis, 'host', 443), [preconnectThis, ['host', 443]]);
    finish();
    await Promise.resolve();
    assert.equal(nativeCalls, 5);
    assert.equal(timers, 0);
    assert.equal(rows.filter(row => row[0] === 'CE1').length, 2);
    assert.equal(rows.filter(row => row[0] === 'CO1').length, 1);
    assert.deepEqual(rows.at(-1), ['CX1', 4, 2, 1, 1]);
    const errors = rows.filter(row => row[0] === 'CE1');
    assert.deepEqual(errors.map(row => row.slice(1)), [
      [1, 'reset', 'verify'], [2, 'reset', 'verify'],
    ]);
    for (const row of rows) assert(!row.join(' ').includes('private text'));
  } finally {
    global.setTimeout = originalSetTimeout;
    Promise.prototype.then = originalThen;
  }
}

function preservesOutcomeWhenRecorderFails() {
  const failure = Object.assign(new Error('unreported'), { code: 'ECONNREFUSED' });
  const rows = [];
  const writer = { write(row) { rows.push(row); throw Error('writer failure'); } };
  const native = () => { throw failure; };
  const { proxy, finish } = require('./client_reason_hook.cjs').install(native, writer);
  assert.throws(() => proxy(), error => error === failure);
  assert.doesNotThrow(finish);
  assert.deepEqual(rows.at(-1), ['CX1', 1, 1, 0, 0]);
}

async function main() {
  rejectsWithoutReadingProperties();
  preservesOutcomeWhenRecorderFails();
  await preservesFetchOutcomesAndBoundsReactions();
}

main().catch(() => { process.exitCode = 1; });
