'use strict';
const assert = require('node:assert/strict');

function classificationDoesNotReadErrorContent() {
  const { classify } = require('./client_reason_classifier.cjs');
  const error = {};
  for (const key of ['message', 'stack', 'name', 'code', 'cause'])
    Object.defineProperty(error, key, { get() { throw Error('private accessor'); } });
  assert.deepEqual(classify(error), { outer: 'accessor', cause: 'accessor' });
  const trap = new Proxy({}, { getOwnPropertyDescriptor() { throw Error('private trap'); } });
  assert.deepEqual(classify(trap), { outer: 'trap', cause: 'trap' });
  assert.deepEqual(classify(null), { outer: 'missing', cause: 'missing' });
  const coercion = { [Symbol.toPrimitive]() { throw Error('must not coerce'); } };
  const oversized = Object.defineProperty({}, 'code', { value: 'X'.repeat(65) });
  const unknown = Object.defineProperty({}, 'code', { value: 'unlisted' });
  assert.deepEqual(classify({ code: coercion }), { outer: 'unknown', cause: 'missing' });
  assert.deepEqual(classify(oversized), { outer: 'unknown', cause: 'missing' });
  assert.deepEqual(classify(unknown), { outer: 'unknown', cause: 'missing' });
}

async function preservesNativeIdentityAndSettlesObservedPhases() {
  const events = [];
  const rows = [];
  let nativeCalls = 0;
  let receiver;
  let receivedArgs;
  let rejectNative;
  const rejection = Object.assign(new Error('private rejection detail'), { code: 'ECONNRESET' });
  const fulfilledValue = Object.freeze({ marker: Symbol('same value') });
  const rejected = new Promise((_, reject) => { rejectNative = reject; });
  function native(...args) {
    nativeCalls++;
    receiver = this;
    receivedArgs = args;
    events.push(`native:${nativeCalls}`);
    if (nativeCalls === 1) throw rejection;
    if (nativeCalls === 2) return rejected;
    return Promise.resolve(fulfilledValue);
  }
  native.preconnect = function (...args) { return [this, args]; };
  const writer = { write(row) { rows.push([...row]); events.push(`write:${row[0]}:${row[1] ?? ''}`); return true; } };
  const { install } = require('./client_reason_hook.cjs');
  const { proxy, finish } = install(native, writer);
  const thisArg = { sentinel: true };
  const args = ['/private/path', { method: 'GET' }];
  assert.throws(() => Reflect.apply(proxy, thisArg, args), error => error === rejection);
  assert.equal(receiver, thisArg);
  assert.equal(receivedArgs[0], args[0]);
  assert.equal(receivedArgs[1], args[1]);
  assert.deepEqual(rows.slice(0, 3), [['CI2', 1], ['CS2', 1], ['CE2', 1, 'reset', 'missing']]);
  assert(events.indexOf('write:CI2:1') < events.indexOf('native:1'));
  assert(events.indexOf('native:1') < events.indexOf('write:CS2:1'));

  const rejectedResult = Reflect.apply(proxy, thisArg, args);
  rejectNative(rejection);
  await assert.rejects(rejectedResult, error => error === rejection);
  const fulfilledResult = await Reflect.apply(proxy, thisArg, args);
  assert.equal(fulfilledResult, fulfilledValue);
  assert(rows.some(row => row[0] === 'CF2' && row[1] === 3));
  assert(rows.some(row => row[0] === 'CE2' && row[1] === 2));
  assert.deepEqual(native.preconnect.call(thisArg, 'host', 443), [thisArg, ['host', 443]]);
  assert.equal(proxy.preconnect, native.preconnect);
  finish();
  assert.deepEqual(rows.at(-1), ['CX2', 3, 2, 0, 1]);
  assert.equal(nativeCalls, 3);
  for (const row of rows) assert(!row.join(' ').includes('private rejection detail'));
}

async function reportsAttachmentFaultAndReturnsOriginalNonPromise() {
  const rows = [];
  const value = Object.freeze({ marker: true });
  const native = () => value;
  const { proxy, finish } = require('./client_reason_hook.cjs').install(native, {
    write(row) { rows.push([...row]); return true; },
  });
  assert.equal(proxy(), value);
  finish();
  assert.deepEqual(rows, [
    ['CI2', 1], ['CN2', 1], ['CA2', 1], ['CX2', 1, 0, 0, 0],
  ]);
}

function recorderFailureCannotSuppressNativeCallOrReplaceItsError() {
  const failure = Object.assign(new Error('native identity'), { code: 'ECONNREFUSED' });
  let calls = 0;
  const writer = { write() { throw Error('writer failure'); } };
  const native = () => { calls++; throw failure; };
  const { proxy, finish } = require('./client_reason_hook.cjs').install(native, writer);
  assert.throws(() => proxy(), error => error === failure);
  assert.equal(calls, 1);
  assert.doesNotThrow(finish);
}

function classifierFailureIsObservedWithoutReplacingNativeError() {
  const classifierPath = require.resolve('./client_reason_classifier.cjs');
  const hookPath = require.resolve('./client_reason_hook.cjs');
  const classifierModule = require(classifierPath);
  const originalClassify = classifierModule.classify;
  const originalHook = require.cache[hookPath];
  const failure = Object.assign(new Error('native identity'), { code: 'ECONNRESET' });
  const rows = [];
  classifierModule.classify = () => { throw Error('observer fault'); };
  delete require.cache[hookPath];
  try {
    const native = () => { throw failure; };
    const { proxy, finish } = require(hookPath).install(native, {
      write(row) { rows.push([...row]); return true; },
    });
    assert.throws(() => proxy(), error => error === failure);
    finish();
    assert.deepEqual(rows, [
      ['CI2', 1], ['CS2', 1], ['CU2', 1], ['CX2', 1, 0, 0, 0],
    ]);
  } finally {
    classifierModule.classify = originalClassify;
    if (originalHook) require.cache[hookPath] = originalHook;
    else delete require.cache[hookPath];
  }
}

async function capsObservedCallsAndForwardsPreconnectProperties() {
  const rows = [];
  let timers = 0;
  const originalSetTimeout = global.setTimeout;
  global.setTimeout = (...args) => { timers++; return originalSetTimeout(...args); };
  try {
    const nativePromises = [];
    function native(value) {
      const promise = Promise.resolve(value);
      nativePromises.push(promise);
      return promise;
    }
    const preconnect = function (...args) { return [this, args]; };
    native.preconnect = preconnect;
    const { proxy, finish } = require('./client_reason_hook.cjs').install(native, {
      write(row) { rows.push([...row]); return true; },
    });
    const thisArg = { marker: 1 };
    assert.equal(proxy.preconnect, preconnect);
    assert.deepEqual(proxy.preconnect.call(thisArg, 'host', 443), [thisArg, ['host', 443]]);
    for (let call = 1; call <= 4; call++) await Reflect.apply(proxy, thisArg, [call]);
    const fifthResult = Reflect.apply(proxy, thisArg, [5]);
    assert.equal(fifthResult, nativePromises[4]);
    await fifthResult;
    finish();
    assert.deepEqual(rows.filter(row => row[0] === 'CI2').map(row => row[1]), [1, 2, 3, 4]);
    assert.deepEqual(rows.filter(row => row[0] === 'CO2'), [['CO2']]);
    assert(!rows.some(row => ['CI2', 'CN2', 'CS2', 'CE2', 'CF2', 'CA2', 'CU2'].includes(row[0]) && row[1] === 5));
    assert.deepEqual(rows.at(-1), ['CX2', 4, 0, 1, 1]);
    assert.equal(timers, 0);
  } finally {
    global.setTimeout = originalSetTimeout;
  }
}

async function main() {
  classificationDoesNotReadErrorContent();
  recorderFailureCannotSuppressNativeCallOrReplaceItsError();
  classifierFailureIsObservedWithoutReplacingNativeError();
  await preservesNativeIdentityAndSettlesObservedPhases();
  await reportsAttachmentFaultAndReturnsOriginalNonPromise();
  await capsObservedCallsAndForwardsPreconnectProperties();
}

main().catch(() => { process.exitCode = 1; });
