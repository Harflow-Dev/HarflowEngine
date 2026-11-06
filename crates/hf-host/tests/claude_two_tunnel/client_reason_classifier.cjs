'use strict';
const descriptor = Object.getOwnPropertyDescriptor;
const own = (value, key) => {
  if ((typeof value !== 'object' || value === null) && typeof value !== 'function')
    return { kind: 'missing' };
  try {
    const d = descriptor(value, key);
    if (!d) return { kind: 'missing' };
    if (!descriptor(d, 'value')) return { kind: 'accessor' };
    return { kind: 'data', value: d.value };
  } catch (_) { return { kind: 'trap' }; }
};
const entries = [
  [['UNABLE','TO','VERIFY','LEAF','SIGNATURE'], 'verify'],
  [['CERTIFICATE','VERIFY','FAILED'], 'verify'],
  [['UNABLE','TO','GET','ISSUER','CERT','LOCALLY'], 'issuer'],
  [['DEPTH','ZERO','SELF','SIGNED','CERT'], 'selfsigned'],
  [['SELF','SIGNED','CERT','IN','CHAIN'], 'selfsigned'],
  [['CERT','HAS','EXPIRED'], 'expired'],
  [['ERR','TLS','CERT','ALTNAME','INVALID'], 'hostname'],
  [['CERT','NOT','YET','VALID'], 'not_yet_valid'],
  [['EPROTO'], 'protocol'], [['ETIMEDOUT'], 'timeout'], [['ABORT_ERR'], 'cancelled'],
  [['EPIPE'], 'broken_pipe'], [['ENOTCONN'], 'not_connected'],
  [['ECONNRESET'], 'reset'], [['ECONNREFUSED'], 'refused'],
  [['ERR','SSL','CERTIFICATE','VERIFY','FAILED'], 'verify']
].map(([parts, category]) => [parts.join('_'), category]);
function code(value) {
  const d = own(value, 'code');
  if (d.kind !== 'data') return d.kind;
  if (typeof d.value !== 'string' || d.value.length > 64) return 'unknown';
  for (let i = 0; i < entries.length; i++)
    if (d.value === entries[i][0]) return entries[i][1];
  return 'unknown';
}
function classify(error) {
  const cause = own(error, 'cause');
  return { outer: code(error), cause: cause.kind === 'data' ? code(cause.value) : cause.kind };
}
module.exports = { classify };
