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
      emit(['CE2', id, c.outer, c.cause]);
    } catch (_) { healthy = false; emit(['CU2', id]); }
  }
  const proxy = new ProxyCtor(native, {
    apply(target, receiver, args) {
      if (calls === 4) {
        if (!overflow) { overflow = true; emit(['CO2']); }
        return apply(target, receiver, args);
      }
      const id = ++calls;
      emit(['CI2', id]);
      let result;
      try { result = apply(target, receiver, args); }
      catch (error) { emit(['CS2', id]); observe(error, id); throw error; }
      emit(['CN2', id]);
      try {
        return apply(then, result, [
          value => { emit(['CF2', id]); return value; },
          error => { observe(error, id); throw error; }
        ]);
      } catch (_) { healthy = false; emit(['CA2', id]); return result; }
    }
  });
  return { proxy, finish() { emit(['CX2', calls, records, overflow ? 1 : 0, healthy ? 1 : 0]); } };
}
module.exports = { install };
