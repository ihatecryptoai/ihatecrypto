// node Tools/updates/worker.test.mjs — the feed's update count: the version read from Sparkle's User-Agent, nothing
// counted for anyone else, and the feed answered even when D1 fails. Not deployed (wrangler takes worker.js and assets).
import assert from 'node:assert/strict';
import worker from './worker.js';

async function get(ua, db) {
  const rows = [], waits = [];
  const env = {
    ASSETS: { fetch: async () => new Response('<rss/>') },
    DB: db ?? { prepare: () => ({ bind: (...v) => ({ run: async () => rows.push(v) }) }) },
  };
  const res = await worker.fetch(new Request('https://updates.ihatecrypto.ai/appcast.xml', { headers: ua ? { 'user-agent': ua } : {} }),
    env, { waitUntil: (p) => waits.push(p) });
  await Promise.all(waits);
  return { body: await res.text(), versions: rows.map((r) => r[1]) };
}

assert.deepEqual(await get('ihatecrypto/0.8.0 Sparkle/2.10.0'), { body: '<rss/>', versions: ['0.8.0'] });
assert.deepEqual((await get('ihatecrypto/0.8.0b1 Sparkle/2.10.0')).versions, ['other']);
assert.deepEqual((await get('Mozilla/5.0 (Macintosh)')).versions, []);
assert.deepEqual((await get(null)).versions, []);
assert.equal((await get('ihatecrypto/0.8.0 Sparkle/2.10.0', { prepare: () => { throw new Error('no D1'); } })).body, '<rss/>');
console.log('worker: ok');
