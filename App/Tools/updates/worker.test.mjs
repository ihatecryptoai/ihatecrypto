// node Tools/updates/worker.test.mjs — the feed's update count: the version read from Sparkle's User-Agent, nothing
// counted for anyone else, and the feed answered even when D1 fails; /stats drawn with noindex, and without the sources
// that fail. Not deployed (wrangler takes worker.js, stats.js and assets).
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

// /stats: D1, GitHub and Hyperliquid mocked; a referred account's address never reaches the page.
const db = { prepare: (sql) => sql, batch: async () => [
  { results: [{ name: 'download', n: 57, first: '2026-10-10' }, { name: 'story_1', n: 36, first: '2026-10-10' }] },
  { results: [{ day: new Date().toISOString().slice(0, 10), mac: 1, n: 3 }] },
  { results: [{ device: 'mac', n: 50 }, { device: 'other', n: 7 }] },
  { results: [{ day: new Date().toISOString().slice(0, 10), version: '0.8.0', n: 2 }] },
  { results: [{ lang: 'en', device: 'mac', n: 30 }, { lang: 'ru', device: 'iphone', n: 6 }] },
] };
const referred = '0x' + 'ab'.repeat(20);
async function page(host, fetcher) {
  globalThis.fetch = fetcher;
  const res = await worker.fetch(new Request(`https://${host}/stats`), { ASSETS: { fetch: async () => new Response('asset', { status: 404 }) }, DB: db },
    { waitUntil: () => {} });
  return { status: res.status, robots: res.headers.get('x-robots-tag'), body: await res.text() };
}
const sources = async (url) => String(url).includes('hyperliquid')
  ? Response.json({ builderRewards: '0.025', claimedRewards: '0', unclaimedRewards: '0.027',
      referrerState: { data: { nReferrals: 1, referralStates: [{ user: referred, cumVlm: '73.85', cumFeesRewardedToReferrer: '0.002' }] } } })
  : Response.json(String(url).includes('/releases') ? [{ assets: [{ name: 'a.dmg', download_count: 4 }] }] : { stargazers_count: 5, forks_count: 1 });
const ok = await page('ihatecrypto.ai', sources);
assert.equal(ok.status, 200);
assert.equal(ok.robots, 'noindex, nofollow');
assert.match(ok.body, /<meta name="robots" content="noindex, nofollow">/);
assert.match(ok.body, /<b>50<\/b><span>Downloads on a Mac/);   // only the Mac's: the DMG runs nowhere else
assert.match(ok.body, /Also fetched from other \(scripts, link scanners\) 7\. ihatecrypto runs only on a Mac/);
assert.match(ok.body, /\$50\.00 traded through the app/);
assert.ok(!ok.body.toLowerCase().includes(referred));
const down = await page('ihatecrypto.ai', async () => { throw new Error('offline'); });
assert.equal(down.status, 200);
assert.match(down.body, /GitHub didn’t answer just now: offline/);
assert.match(down.body, /<b>50<\/b>/);
assert.equal((await page('updates.ihatecrypto.ai', sources)).body, 'asset');
console.log('worker: ok');
