'use strict';
const { classify } = require('./client_reason_classifier.cjs');
const apply = Reflect.apply;
const then = Promise.prototype.then;
const ProxyCtor = Proxy;
function install(native, writer) {
  let calls = 0, records = 0, overflow = false, healthy = true;
  function emit(row) {
    try { if (!writer.write(row)) healthy = false; } catch (_) { healthy = false; }
  }
  function observe(error, id) {
    try {
      const c = classify(error);
      records++;
      emit(['CE1', id, c.outer, c.cause]);
    } catch (_) { healthy = false; }
  }
  const proxy = new ProxyCtor(native, {
    apply(target, receiver, args) {
      if (calls === 4) {
        if (!overflow) { overflow = true; emit(['CO1']); }
        return apply(target, receiver, args);
      }
      const id = ++calls;
      let result;
      try { result = apply(target, receiver, args); }
      catch (error) { observe(error, id); throw error; }
      try {
        return apply(then, result, [undefined, error => { observe(error, id); throw error; }]);
      } catch (_) { healthy = false; return result; }
    }
  });
  return { proxy, finish() { emit(['CX1', calls, records, overflow ? 1 : 0, healthy ? 1 : 0]); } };
}
module.exports = { install };
