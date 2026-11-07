-- The landing's counts (D1 «ihatecrypto-landing»): how many times something happened, by day, language and kind of
-- device. No IPs, no ids, no people: only how many.
--   npx wrangler d1 execute ihatecrypto-landing --remote --file Tools/updates/schema.sql
--   npx wrangler d1 execute ihatecrypto-landing --remote --command "SELECT name, SUM(n) FROM events GROUP BY name ORDER BY 2 DESC"
CREATE TABLE IF NOT EXISTS events (
  day TEXT NOT NULL,      -- YYYY-MM-DD, UTC
  name TEXT NOT NULL,     -- download, send, share_tg …, story_1, story_2, story_end, seen_canvas …, faq_open, menu, lang
  lang TEXT NOT NULL,     -- en, es, ru, other
  device TEXT NOT NULL,   -- mac, iphone, ipad, android, windows, other
  n INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, name, lang, device)
);

-- One row per day and app version: how many times Sparkle asked for appcast.xml (worker.js). The version is what
-- Sparkle's User-Agent names, «other» when it names none we read.
--   npx wrangler d1 execute ihatecrypto-landing --remote --command "SELECT day, version, n FROM update_checks ORDER BY day DESC, n DESC"
CREATE TABLE IF NOT EXISTS update_checks (
  day TEXT NOT NULL,      -- YYYY-MM-DD, UTC
  version TEXT NOT NULL,  -- 0.8.0 …, other
  n INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, version)
);
