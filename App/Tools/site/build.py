#!/usr/bin/env python3
"""The landing in three languages, as search engines read it: index.html (English, the page as written) and /es/, /ru/
rendered from the same page with its own words (the i18n block): the text, the title and description, the previews,
the FAQ for search results, and hreflang links tying the three together. The manifesto (manifesto.html) the same way,
at /manifesto/, /es/manifesto/ and /ru/manifesto/. What's new (whatsnew.html) at /whats-new/: every release from the
app's own list (Sources/IhatecryptoApp/WhatsNew/whatsnew.json), in English like the app's.

    python3 Tools/site/build.py <version> <out dir>      (release.sh runs it; out is dist/site)
"""
import html
import json
import re
import sys
from datetime import date
from pathlib import Path

SITE = 'https://ihatecrypto.ai/'
LANGS = {'en': ('', 'en_US'), 'es': ('es/', 'es_ES'), 'ru': ('ru/', 'ru_RU')}


def render(page: str, lang: str, T: dict) -> str:
    path, locale = LANGS[lang]
    words = T.get(lang, {})
    # English is what the page says; a key without a translation keeps it.
    en = {k: html.unescape(v) for k, v in re.findall(r'data-t="(\w+)"[^>]*>([^<]*)<', page)}

    def say(key):
        return words.get(key, en.get(key, ''))

    # hreflang: every page names all three and the default
    links = ''.join(f'<link rel="alternate" hreflang="{l}" href="{SITE}{p}">\n' for l, (p, _) in LANGS.items())
    page = page.replace('<link rel="canonical" href="https://ihatecrypto.ai/">',
                        f'<link rel="canonical" href="{SITE}{path}">\n{links}<link rel="alternate" hreflang="x-default" href="{SITE}">', 1)
    if lang == 'en':
        return page

    # the page's script takes English from the page's own words, which here are this language's: hand it English whole,
    # or switching to English from /ru/ or /es/ keeps this language
    ta = {k: html.unescape(v) for k, v in re.findall(r'\bdata-ta="(\w+)"[^>]*?aria-label="([^"]*)"', page)}
    m = re.search(r'(<script type="application/json" id="i18n">\n)(.*?)(\n</script>)', page, re.S)
    words_all = json.loads(m.group(2))
    words_all['en'] = {**en, **ta, **words_all['en']}
    page = page[:m.start(2)] + json.dumps(words_all, ensure_ascii=False, indent=0).replace('</', '<\\/') + page[m.end(2):]

    # the text: every data-t element holds plain words, so its words are swapped whole
    page = re.sub(r'(<(\w+)\b[^>]*?\bdata-t="(\w+)"[^>]*>)[^<]*(</\2>)',
                  lambda m: m.group(1) + html.escape(say(m.group(3)), quote=False) + m.group(4) if m.group(3) in words else m.group(0), page)
    page = re.sub(r'(\bdata-ta="(\w+)"[^>]*?aria-label=")[^"]*(")',
                  lambda m: m.group(1) + html.escape(say(m.group(2))) + m.group(3) if m.group(2) in words else m.group(0), page)

    # the head: language, title, description, previews
    title, desc, og = say('meta_title'), say('meta_desc'), f"ihatecrypto — {say('slogan')}"
    page = page.replace('<html lang="en">', f'<html lang="{lang}" data-lang="{lang}">', 1)
    page = page.replace('href="/manifesto/"', f'href="/{path}manifesto/"')
    page = re.sub(r'<title>[^<]*</title>', f'<title>{html.escape(title)}</title>', page, 1)
    for attr, value in [('name="description"', desc), ('property="og:title"', og), ('name="twitter:title"', og),
                        ('property="og:description"', desc), ('name="twitter:description"', desc)]:
        page = re.sub(rf'(<meta {attr} content=")[^"]*(")', lambda m: m.group(1) + html.escape(value) + m.group(2), page, 1)
    page = page.replace(f'<meta property="og:url" content="{SITE}">', f'<meta property="og:url" content="{SITE}{path}">', 1)
    others = [loc for l, (_, loc) in LANGS.items() if l != lang]
    page = re.sub(r'<meta property="og:locale" content="[^"]*">\n<meta property="og:locale:alternate" content="[^"]*">\n<meta property="og:locale:alternate" content="[^"]*">',
                  f'<meta property="og:locale" content="{locale}">\n' + '\n'.join(f'<meta property="og:locale:alternate" content="{o}">' for o in others), page, 1)

    # the FAQ search results show: the same questions, in this language
    m = re.search(r'(<script type="application/ld\+json">\n)(.*?)(\n</script>)', page, re.S)
    data = json.loads(m.group(2))
    by_en = {v: k for k, v in en.items()}
    for node in data['@graph']:
        if node['@type'] == 'SoftwareApplication':
            node['description'] = desc
            node['url'] = SITE + path
        if node['@type'] == 'FAQPage':
            for q in node['mainEntity']:
                q['name'] = say(by_en.get(q['name'], '')) or q['name']
                q['acceptedAnswer']['text'] = say(by_en.get(q['acceptedAnswer']['text'], '')) or q['acceptedAnswer']['text']
    page = page[:m.start(2)] + json.dumps(data, ensure_ascii=False, indent=1) + page[m.end(2):]
    return page


def enclave(page: str, url: str) -> str:
    """The Secure Enclave in the text opens Apple's page about it, in the page's language (the page's script does the same
    on a language switch)."""
    return re.sub(r'(\bdata-t="\w+"[^>]*>[^<]*)Secure Enclave',
                  lambda m: m.group(1) + f'<a href="{url}" target="_blank" rel="noopener">Secure Enclave</a>', page)


def manifesto(page: str, lang: str) -> str:
    """manifesto.html in one language: its words sit in <template lang="…">, this language's stay and the others go.
    A Russian one-letter word is tied to the next one by a no-break space, as on the landing."""
    path, _ = LANGS[lang]
    page = re.sub(r'<template lang="(\w+)">(.*?)</template>', lambda m: m.group(2) if m.group(1) == lang else '', page, flags=re.S)
    links = ''.join(f'<link rel="alternate" hreflang="{l}" href="{SITE}{p}manifesto/">\n' for l, (p, _) in LANGS.items())
    page = page.replace('<link rel="canonical">', f'<link rel="canonical" href="{SITE}{path}manifesto/">\n{links}'
                        f'<link rel="alternate" hreflang="x-default" href="{SITE}manifesto/">', 1)
    page = page.replace('<html lang="en">', f'<html lang="{lang}">', 1)
    page = page.replace('{{lang}}', lang).replace('{{LANG}}', lang.upper()).replace('{{home}}', '/' + path)
    if lang == 'ru':
        page = re.sub(r'(?<![\w-])([ВвИиСсКкАаОоУуЯя]) ', '\\1\u00a0', page)
    return page


def whatsnew(page: str, landing: str, releases: list) -> str:
    """whatsnew.html with every release, newest first, from the app's own What's New: a version, its date and its changes,
    each with the phrase the app's «Try it» puts into the chat. English only, like the app's list. The fonts and the
    coin are the landing's own, so there is one copy of each."""
    def story(s):
        say = f'<p class="try">{html.escape(s["try"])}</p>' if s.get('try') else ''
        return (f'<article class="st"><h3><i style="background:#{html.escape(s["color"])}"></i>{html.escape(s["title"])}</h3>'
                f'<p>{html.escape(s["text"])}</p>{say}</article>')

    def release(r, latest):
        day = date.fromisoformat(r['date'])
        mark = ' <span class="now">Latest</span>' if latest else ''   # the space: read aloud it is «0.8.5 Latest»
        return (f'<section class="rel" id="v{html.escape(r["version"])}"><header><h2>{html.escape(r["version"])}{mark}</h2>'
                f'<time datetime="{day.isoformat()}">{day:%B} {day.day}, {day.year}</time></header>\n'
                f'<div class="sts">{"".join(story(s) for s in r["stories"])}</div></section>')

    coin = re.search(r'<symbol id="f-wait" viewBox="([^"]*)">(.*?)</symbol>', landing, re.S)
    page = page.replace('{{fonts}}', ''.join(re.findall(r'  @font-face \{[^}]*\}\n', landing)))
    page = page.replace('{{coin}}', f'<svg viewBox="{coin.group(1)}" aria-hidden="true">{coin.group(2)}</svg>')
    return page.replace('{{releases}}', '\n'.join(release(r, i == 0) for i, r in enumerate(releases)))


def main():
    version, out = sys.argv[1], Path(sys.argv[2])
    here = Path(__file__).parent
    releases = json.loads((here.parent.parent / 'Sources/IhatecryptoApp/WhatsNew/whatsnew.json').read_text(encoding='utf-8'))
    news = whatsnew((here / 'whatsnew.html').read_text(encoding='utf-8').replace('{{version}}', version),
                    (here / 'index.html').read_text(encoding='utf-8'), releases)
    (out / 'whats-new').mkdir(parents=True, exist_ok=True)
    (out / 'whats-new' / 'index.html').write_text(news, encoding='utf-8')
    print(out / 'whats-new' / 'index.html')
    page = (here / 'index.html').read_text(encoding='utf-8').replace('{{version}}', version)
    mf = (here / 'manifesto.html').read_text(encoding='utf-8').replace('{{version}}', version)
    T = json.loads(re.search(r'<script type="application/json" id="i18n">\n(.*?)\n</script>', page, re.S).group(1))
    for lang, (path, _) in LANGS.items():
        for target, text in [(out / path / 'index.html', enclave(render(page, lang, T), T[lang]['se'])),
                             (out / path / 'manifesto' / 'index.html', manifesto(mf, lang))]:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding='utf-8')
            print(target)


if __name__ == '__main__':
    main()
