// The landing's pages, its downloads and its counts go through here first (wrangler.jsonc's run_worker_first); the
// update feed, the zips and every other file are served straight from the static assets, free and without a limit.
//   http → https and www → the bare domain, 301; the pages carry HSTS, so a browser asks over https from then on.
//   POST /e {n, l}: the page says what was pressed or reached; GET /ihatecrypto.dmg counts as a download.
// Counts are added up by day, language and kind of device in D1 (schema.sql). No IP, no cookie, no id is kept.
const EVENTS = new Set(['send', 'share_tg', 'share_wa', 'share_mail', 'share_x', 'share_copy', 'story_1', 'story_2', 'story_end',
  'seen_canvas', 'seen_how', 'seen_safe', 'seen_open', 'seen_ask', 'seen_faq', 'faq_open', 'menu', 'lang', 'claude', 'github']);
const LANGS = new Set(['en', 'es', 'ru']);
const SITE = new Set(['ihatecrypto.ai', 'www.ihatecrypto.ai']);

const device = (ua = '') => (/iPhone|iPod/.test(ua) ? 'iphone' : /iPad/.test(ua) ? 'ipad' : /Android/.test(ua) ? 'android'
  : /Macintosh/.test(ua) ? 'mac' : /Windows/.test(ua) ? 'windows' : 'other');
const langOf = (l) => (LANGS.has(l) ? l : 'other');

function count(env, name, lang, request) {
  return env.DB.prepare('INSERT INTO events (day, name, lang, device, n) VALUES (?1, ?2, ?3, ?4, 1) '
    + 'ON CONFLICT (day, name, lang, device) DO UPDATE SET n = n + 1')
    .bind(new Date().toISOString().slice(0, 10), name, lang, device(request.headers.get('user-agent') ?? ''))
    .run().catch(() => {});   // a count that didn't land never breaks the page
}

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const site = SITE.has(url.hostname);
    if (site && (url.protocol === 'http:' || url.hostname !== 'ihatecrypto.ai')) {
      url.protocol = 'https:';
      url.hostname = 'ihatecrypto.ai';
      return Response.redirect(url.toString(), 301);
    }
    if (url.pathname === '/e') {
      if (request.method === 'POST') {
        let body = {};
        try { body = JSON.parse(await request.text()); } catch {}
        if (EVENTS.has(body.n)) ctx.waitUntil(count(env, body.n, langOf(body.l), request));
      }
      return new Response(null, { status: 204 });
    }
    // A download once: the first byte asked for, not each piece of a resumed one.
    const range = request.headers.get('range');
    if (url.pathname === '/ihatecrypto.dmg' && request.method === 'GET' && (!range || /^bytes=0-/.test(range))) {
      ctx.waitUntil(count(env, 'download', langOf((request.headers.get('accept-language') ?? '').slice(0, 2).toLowerCase()), request));
    }
    const res = await env.ASSETS.fetch(request);
    if (!site) return res;
    const out = new Response(res.body, res);
    out.headers.set('Strict-Transport-Security', 'max-age=31536000');
    return out;
  },
};
