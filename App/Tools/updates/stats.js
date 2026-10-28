// GET ihatecrypto.ai/stats (worker.js): the project's numbers on one page, read live each time it's asked for. Nothing
// on the site links to it and it carries noindex. Each block says where its numbers come from; a source that doesn't
// answer leaves its block saying so and the rest of the page as it is.
const BUILDER = '0x3828b920d705587b16be05d2d3764333a58c87ec';   // HLBuilder.address, HyperliquidSigning.swift
const BUILDER_SHARE = 0.0005;                                    // HLBuilder.share: 0.05% of each side of a trade
const REPO = 'ihatecryptoai/ihatecrypto';

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const int = (n) => Number(n ?? 0).toLocaleString('en-US');
const usd = (n) => '$' + Number(n).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: n < 1 ? 4 : 2 });
const days = (count) => Array.from({ length: count }, (_, i) => new Date(Date.now() - (count - 1 - i) * 864e5).toISOString().slice(0, 10));

async function counts(env) {
  const q = (sql) => env.DB.prepare(sql);
  const [byName, downloads, byDevice, checks, story] = await env.DB.batch([
    q('SELECT name, SUM(n) n, MIN(day) first FROM events GROUP BY name'),
    q("SELECT day, SUM(CASE WHEN device = 'mac' THEN n ELSE 0 END) mac, SUM(n) n FROM events WHERE name = 'download' "
      + "AND day >= date('now', '-29 days') GROUP BY day"),
    q("SELECT device, SUM(n) n FROM events WHERE name = 'download' GROUP BY device ORDER BY n DESC"),
    q("SELECT day, version, n FROM update_checks WHERE day >= date('now', '-29 days') ORDER BY day DESC, n DESC"),
    q("SELECT lang, device, SUM(n) n FROM events WHERE name = 'story_1' GROUP BY lang, device"),
  ]);
  return { byName: byName.results, downloads: downloads.results, byDevice: byDevice.results, checks: checks.results, story: story.results };
}

async function github() {
  const get = (path) => fetch(`https://api.github.com/repos/${REPO}${path}`, {
    headers: { 'user-agent': 'ihatecrypto.ai stats', accept: 'application/vnd.github+json' },
    cf: { cacheTtl: 900, cacheEverything: true },   // GitHub allows 60 unsigned requests an hour per address
  }).then((r) => (r.ok ? r.json() : Promise.reject(new Error(`GitHub answered ${r.status}`))));
  const [repo, releases] = await Promise.all([get(''), get('/releases?per_page=100')]);
  const dmg = releases.flatMap((r) => r.assets).filter((a) => a.name.endsWith('.dmg'));
  return { stars: repo.stargazers_count, forks: repo.forks_count, releases: releases.length, dmg: dmg.reduce((s, a) => s + a.download_count, 0) };
}

async function hyperliquid() {
  const r = await fetch('https://api.hyperliquid.xyz/info', {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ type: 'referral', user: BUILDER }),
  });
  if (!r.ok) throw new Error(`Hyperliquid answered ${r.status}`);
  const d = await r.json();
  const referred = d.referrerState?.data?.referralStates ?? [];   // other people's accounts: only sums leave this function
  const builder = Number(d.builderRewards ?? 0);
  return {
    builder, volume: builder / BUILDER_SHARE, earned: Number(d.claimedRewards ?? 0) + Number(d.unclaimedRewards ?? 0),
    referral: referred.reduce((s, x) => s + Number(x.cumFeesRewardedToReferrer ?? 0), 0),
    referredVolume: referred.reduce((s, x) => s + Number(x.cumVlm ?? 0), 0), accounts: d.referrerState?.data?.nReferrals ?? referred.length,
  };
}

const LABELS = [
  ['story_1', 'Story, first step reached'], ['story_2', 'Story, second step reached'], ['story_end', 'Story, end reached'],
  ['seen_canvas', 'Saw «A canvas like Figma»'], ['seen_how', 'Saw «How it works»'], ['seen_safe', 'Saw «Why it’s safe»'],
  ['seen_open', 'Saw «Open source»'], ['seen_ask', 'Saw the examples'], ['seen_faq', 'Saw the questions'], ['faq_open', 'Opened an answer'],
  ['github', 'Went to GitHub'], ['claude', 'Pressed «Check with Claude»'], ['send', 'Pressed «Send to Mac»'],
  ['share_tg', 'Shared to Telegram'], ['share_wa', 'Shared to WhatsApp'], ['share_mail', 'Shared by mail'], ['share_x', 'Shared to X'],
  ['share_copy', 'Copied the link'], ['menu', 'Opened the phone menu'], ['lang', 'Picked a language'],
];

// One bar a day; with `mac` set, the Mac part in the accent and the rest muted on top of it.
function bars(rows, total) {
  const max = Math.max(1, ...rows.map((r) => r.n));
  const h = (n) => `${Math.max(n ? 4 : 0, Math.round(n / max * 100))}%`;
  return `<div class="bars" role="img" aria-label="${esc(total)}">${rows.map((r) => {
    const mac = r.mac ?? r.n;
    return `<div class="bar" title="${esc(r.day)}: ${int(mac)}${r.mac === undefined ? '' : ` on a Mac, ${int(r.n - mac)} elsewhere`}">`
      + (r.n > mac ? `<i class="rest" style="height:${h(r.n - mac)}"></i>` : '') + (mac ? `<i style="height:${h(mac)}"></i>` : '') + '</div>';
  }).join('')}</div>`
    + `<div class="axis"><span>${esc(rows[0].day)}</span><span>${esc(rows.at(-1).day)}</span></div>`;
}

// The DMG asked for from anything but a Mac: a Windows or phone browser, a link scanner, a script. Not installs.
const ELSEWHERE = { windows: 'Windows browsers', iphone: 'iPhone', ipad: 'iPad', android: 'Android', other: 'other (scripts, link scanners)' };
function elsewhere(byDevice) {
  const rest = byDevice.filter((r) => r.device !== 'mac' && r.n);
  if (!rest.length) return '';
  return `<p class="note"><span class="swatch"></span>Also fetched from ${rest.map((r) => `${esc(ELSEWHERE[r.device] ?? r.device)} ${int(r.n)}`).join(', ')}. `
    + 'ihatecrypto runs only on a Mac, so these aren’t installs.</p>';
}

const failed = (what, reason) => `<p class="miss">${esc(what)} didn’t answer just now: ${esc(reason?.message ?? reason)}. Reload in a minute.</p>`;

export async function stats(env) {
  const [c, gh, hl] = await Promise.allSettled([counts(env), github(), hyperliquid()]);
  const read = new Date().toISOString().replace('T', ' ').slice(0, 16) + ' UTC';
  const name = c.value ? Object.fromEntries(c.value.byName.map((r) => [r.name, r])) : {};
  const total = (k) => name[k]?.n ?? 0;
  const month = days(30);

  const byDay = (rows) => { const m = Object.fromEntries(rows.map((r) => [r.day, r])); return month.map((day) => ({ ...m[day], day, n: m[day]?.n ?? 0 })); };
  const onMac = c.value?.byDevice.find((r) => r.device === 'mac')?.n ?? 0;
  let checkDays = [], yesterday = 0;
  if (c.value) {
    const sum = {};
    for (const r of c.value.checks) sum[r.day] = (sum[r.day] ?? 0) + r.n;
    checkDays = byDay(Object.entries(sum).map(([day, n]) => ({ day, n })));
    yesterday = sum[month.at(-2)] ?? 0;
  }

  const tiles = [
    [c.value ? int(onMac) : '—', 'Downloads on a Mac', name.download ? `since ${name.download.first}` : ''],
    [c.value ? int(yesterday) : '—', 'Update checks yesterday', 'about one a day per copy in use'],
    [hl.value ? usd(hl.value.earned) : '—', 'Earned on Hyperliquid', hl.value ? `${usd(hl.value.volume)} traded through the app` : ''],
    [gh.value ? int(gh.value.stars) : '—', 'GitHub stars', gh.value ? `${int(gh.value.forks)} forks` : ''],
  ];

  const checksTable = c.value?.checks.length
    ? `<table><thead><tr><th>Day, UTC</th><th>Version</th><th class="n">Checks</th></tr></thead><tbody>${c.value.checks.map((r) =>
      `<tr><td>${esc(r.day)}</td><td>${esc(r.version)}</td><td class="n">${int(r.n)}</td></tr>`).join('')}</tbody></table>`
    : '<p class="miss">No update checks counted yet.</p>';

  const split = (key) => {
    const m = {};
    for (const r of c.value?.story ?? []) m[r[key]] = (m[r[key]] ?? 0) + r.n;
    return Object.entries(m).sort((a, b) => b[1] - a[1]).map(([k, n]) => `<span class="chip">${esc(k)} <b>${int(n)}</b></span>`).join('');
  };

  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="robots" content="noindex, nofollow">
<title>ihatecrypto stats</title>
<link rel="icon" href="/favicon.png">
<style>
  @font-face { font-family: 'Onest'; font-style: normal; font-weight: 400 800; font-display: swap; src: url(/fonts/onest-latin.woff2) format('woff2'); }
  :root { color-scheme: light; --bg: #FAFAFB; --card: #FFFFFF; --ink: #111111; --muted: #5F636B; --line: #E1E4EC; --accent: #FF7A2F; --accent-ink: #B8480C;
    --ui: 'Onest', -apple-system, 'Segoe UI', system-ui, sans-serif; --mono: ui-monospace, 'SF Mono', Menlo, monospace; }
  @media (prefers-color-scheme: dark) { :root { color-scheme: dark; --bg: #0E0F11; --card: #16171A; --ink: #F2F2F3; --muted: #9A9EA6; --line: #2A2C31; --accent-ink: #FF9A5C; } }
  * { box-sizing: border-box; margin: 0; }
  body { background: var(--bg); color: var(--ink); font: 400 15px/1.5 var(--ui); padding: max(28px, env(safe-area-inset-top)) max(16px, env(safe-area-inset-right)) 64px max(16px, env(safe-area-inset-left)); }
  main { max-width: 880px; margin: 0 auto; display: grid; gap: 40px; }
  .eyebrow, th, .axis, .src { font: 500 11px/1.4 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  h1 { font-size: clamp(28px, 5vw, 40px); font-weight: 800; letter-spacing: -.02em; margin-top: 6px; }
  h1 span { color: var(--accent); }
  h2 { font-size: 19px; font-weight: 700; margin-bottom: 4px; }
  .lede { color: var(--muted); margin-top: 6px; max-width: 62ch; }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
  @media (max-width: 760px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  .tile { background: var(--card); border: 1px solid var(--line); border-radius: 14px; padding: 16px; display: grid; gap: 4px; align-content: start; min-width: 0; }
  .tile b { font-size: 28px; font-weight: 800; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
  .tile span { font-weight: 600; }
  .tile small { color: var(--muted); font-size: 13px; }
  section { display: grid; gap: 12px; }
  .src { text-transform: none; letter-spacing: 0; font-size: 12px; }
  .bars { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 3px; height: 120px; align-items: end; border-bottom: 1px solid var(--line); }
  .bar { height: 100%; display: flex; flex-direction: column; justify-content: flex-end; }
  .bar i { display: block; width: 100%; background: var(--accent); }
  .bar i:first-child { border-radius: 3px 3px 0 0; }
  .bar i.rest, .swatch { background: var(--line); }
  .note { color: var(--muted); font-size: 14px; }
  .swatch { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 8px; vertical-align: -1px; }
  .axis { display: flex; justify-content: space-between; text-transform: none; }
  .table { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { text-align: left; padding: 8px 0; border-bottom: 1px solid var(--line); }
  td { font-size: 14px; }
  .n { text-align: right; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip { border: 1px solid var(--line); border-radius: 999px; padding: 3px 10px; font-size: 13px; color: var(--muted); }
  .chip b { color: var(--ink); font-weight: 600; }
  .miss { color: var(--accent-ink); }
  .row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 24px; }
  @media (max-width: 760px) { .row { grid-template-columns: 1fr; } }
</style>
</head>
<body>
<main>
<header>
  <div class="eyebrow">Read live · ${esc(read)}</div>
  <h1>ihatecrypto<span>.</span> stats</h1>
  <p class="lede">What people do with the app and the site, counted without cookies, IP addresses or ids: only how many times something happened.</p>
</header>

<div class="tiles">${tiles.map(([v, t, s]) => `<div class="tile"><b>${esc(v)}</b><span>${esc(t)}</span><small>${esc(s)}</small></div>`).join('')}</div>

<section>
  <h2>Downloads on a Mac, last 30 days</h2>
  ${c.value ? bars(byDay(c.value.downloads).map((r) => ({ ...r, mac: r.mac ?? 0 })), 'Downloads per day on a Mac') + elsewhere(c.value.byDevice) : failed('The count', c.reason)}
  <p class="src">Each GET of ihatecrypto.ai/ihatecrypto.dmg from its first byte (a resumed download counts once), by the browser’s User-Agent.</p>
</section>

<section>
  <h2>Copies in use</h2>
  ${c.value ? (checkDays.length ? bars(checkDays, 'Update checks per day') : '') + `<div class="table">${checksTable}</div>` : failed('The count', c.reason)}
  <p class="src">Sparkle asks updates.ihatecrypto.ai/appcast.xml about once a day from each copy and names its version in the User-Agent. Counted since 2026-10-27.</p>
</section>

<section class="row">
  <div>
    <h2>Money through the app</h2>
    ${hl.value ? `<div class="table"><table><tbody>
      <tr><td>Builder fees, 0.05% of each trade</td><td class="n">${usd(hl.value.builder)}</td></tr>
      <tr><td>Traded through the app</td><td class="n">${usd(hl.value.volume)}</td></tr>
      <tr><td>Referral share</td><td class="n">${usd(hl.value.referral)}</td></tr>
      <tr><td>Accounts on our referral code</td><td class="n">${int(hl.value.accounts)}</td></tr>
      <tr><td>Their volume</td><td class="n">${usd(hl.value.referredVolume)}</td></tr>
    </tbody></table></div>` : failed('Hyperliquid', hl.reason)}
    <p class="src">Hyperliquid’s info API, «referral» for the app’s builder address (HyperliquidSigning.swift). Volume = builder fees ÷ 0.05%.</p>
  </div>
  <div>
    <h2>Open source</h2>
    ${gh.value ? `<div class="table"><table><tbody>
      <tr><td>Stars</td><td class="n">${int(gh.value.stars)}</td></tr>
      <tr><td>Forks</td><td class="n">${int(gh.value.forks)}</td></tr>
      <tr><td>Releases</td><td class="n">${int(gh.value.releases)}</td></tr>
      <tr><td>DMG downloads from GitHub, any device</td><td class="n">${int(gh.value.dmg)}</td></tr>
    </tbody></table></div>` : failed('GitHub', gh.reason)}
    <p class="src">GitHub’s API for ${esc(REPO)}, kept 15 minutes.</p>
  </div>
</section>

<section>
  <h2>The site, all time</h2>
  ${c.value ? `<div class="table"><table><tbody>${LABELS.filter(([k]) => total(k)).map(([k, label]) =>
    `<tr><td>${esc(label)}</td><td class="n">${int(total(k))}</td></tr>`).join('')}</tbody></table></div>
  <div class="chips">${split('device')}</div><div class="chips">${split('lang')}</div>` : failed('The count', c.reason)}
  <p class="src">The landing’s own beacons (POST /e) since 2026-10-10, by kind of device and language for the story’s first step. Page views are in Cloudflare Web Analytics.</p>
</section>
</main>
</body>
</html>`;
  return new Response(html, {
    headers: { 'content-type': 'text/html; charset=utf-8', 'x-robots-tag': 'noindex, nofollow', 'cache-control': 'public, max-age=60' },
  });
}
