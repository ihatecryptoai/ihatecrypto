#!/usr/bin/env python3
"""The landing in three languages, as search engines read it: index.html (English, the page as written) and /es/, /ru/
rendered from the same page with its own words (the i18n block): the text, the title and description, the previews,
the FAQ for search results, and hreflang links tying the three together. The manifesto (manifesto.html) the same way,
at /manifesto/, /es/manifesto/ and /ru/manifesto/. What's new (whatsnew.html) at /whats-new/, /es/whats-new/ and
/ru/whats-new/: every release from the app's own list (Sources/IhatecryptoApp/WhatsNew/whatsnew.json), in Spanish and
Russian from whatsnew.i18n.json (a change with no translation there stays in English, and the build says how many).

    python3 Tools/site/build.py <version> <out dir>      (release.sh runs it; out is dist/site)
"""
import hashlib
import html
import json
import re
import subprocess
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
    page = page.replace('href="/manifesto/"', f'href="/{path}manifesto/"').replace('href="/whats-new/"', f'href="/{path}whats-new/"')
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


# What's new: the page's own words in each language (the changes come from whatsnew.json and whatsnew.i18n.json).
NEWS = {
    'en': {'title': "What's new in ihatecrypto — every release", 'og_title': "What's new in ihatecrypto",
           'desc': 'Every release of ihatecrypto, the AI crypto wallet for Mac and Windows, and what changed in it. The latest is {version}.',
           'og_desc': 'Every release and what changed in it. The latest is {version}.',
           'kicker': "What's new", 'h1': 'Every release, and what changed.',
           'lead': 'The app tells you when a new version is out and installs it only when you say so. This is the same list it shows you.',
           'latest': 'Latest', 'copy': 'Copy', 'n_mf': 'Manifesto', 'n_pl': 'Plugins', 'dl': 'Download', 'get': 'Get the app',
           'home': 'Home', 'rel': 'Releases on GitHub'},
    'es': {'title': 'Novedades de ihatecrypto: todas las versiones', 'og_title': 'Novedades de ihatecrypto',
           'desc': 'Todas las versiones de ihatecrypto, la billetera cripto con IA para Mac y Windows, y qué cambió en cada una. La última es la {version}.',
           'og_desc': 'Todas las versiones y qué cambió en cada una. La última es la {version}.',
           'kicker': 'Novedades', 'h1': 'Todas las versiones y qué cambió.',
           'lead': 'La app te avisa cuando sale una versión nueva y la instala solo cuando tú lo decides. Esta es la misma lista que te muestra.',
           'latest': 'Última', 'copy': 'Copiar', 'n_mf': 'Manifiesto', 'n_pl': 'Plugins', 'dl': 'Descargar', 'get': 'Consíguela',
           'home': 'Inicio', 'rel': 'Versiones en GitHub'},
    'ru': {'title': 'Что нового в ihatecrypto: все версии', 'og_title': 'Что нового в ihatecrypto',
           'desc': 'Все версии ihatecrypto, криптокошелька с ИИ для Mac и Windows, и что изменилось в каждой. Последняя: {version}.',
           'og_desc': 'Все версии и что изменилось в каждой. Последняя: {version}.',
           'kicker': 'Что нового', 'h1': 'Все версии и что в них изменилось.',
           'lead': 'Приложение сообщает о новой версии и ставит её, только когда вы сами решите. Это тот же список, что и в приложении.',
           'latest': 'Последняя', 'copy': 'Скопировать', 'n_mf': 'Манифест', 'n_pl': 'Плагины', 'dl': 'Скачать', 'get': 'Скачать',
           'home': 'Главная', 'rel': 'Релизы на GitHub'},
}
MONTHS = {'es': 'enero febrero marzo abril mayo junio julio agosto septiembre octubre noviembre diciembre'.split(),
          'ru': 'января февраля марта апреля мая июня июля августа сентября октября ноября декабря'.split()}


def story_id(s: dict) -> str:
    """What a change's translation is filed under in whatsnew.i18n.json: its English words. Edit them and the old
    translation no longer applies, so the page shows the new English until it is translated again."""
    return hashlib.sha1((s['title'] + '\n' + s['text']).encode()).hexdigest()[:12]


def windows_version() -> str:
    """The latest Windows release, for the download's menu: the highest windows-v tag of this repository. Every Windows
    release makes one, so nobody has to remember a number here. Without git or the tags it is empty, and the menu says
    «Windows» with no number rather than an old one: a release must not stop over it, so this only leaves a note."""
    try:
        tags = subprocess.run(['git', '-C', str(Path(__file__).parent), 'tag', '-l', 'windows-v*', '--sort=-v:refname'],
                              capture_output=True, text=True).stdout.split()
    except OSError:
        tags = []
    if not tags:
        print("What's new: no windows-v tag here (git fetch --tags), so the download's menu names no Windows version", file=sys.stderr)
    return tags[0].removeprefix('windows-v') if tags else ''


def translated(words: dict, s: dict):
    """This language's title and text of a change, or None when it has none (or an empty one): then the English stands."""
    w = words.get(story_id(s))
    return w if w and w.get('title') and w.get('text') else None


def whatsnew(page: str, landing: str, releases: list, lang: str, words: dict, version: str) -> str:
    """whatsnew.html in one language with every release, newest first, from the app's own What's New: a version, its
    date and its changes, each with the phrase the app's «Try it» puts into the chat, copied by a press. Its download
    names the latest version for each computer: this release for the Mac, the newest windows-v tag for a PC. The phrase
    stays as the app has it: that is the one known to work. `words` are this language's translations by story_id; a
    change without one stays in English. The fonts and the coin are the landing's own, so there is one copy of each."""
    path, _ = LANGS[lang]
    t = {k: v.replace('{version}', version) for k, v in NEWS[lang].items()}

    def story(s):
        w = translated(words, s) or s
        say = (f'<button type="button" class="try" aria-label="{html.escape(t["copy"])}: {html.escape(s["try"])}"><span>{html.escape(s["try"], quote=False)}</span>'
               '<svg class="i1" aria-hidden="true"><use href="#i-copy"/></svg><svg class="i2" aria-hidden="true"><use href="#i-ok"/></svg></button>') if s.get('try') else ''
        return (f'<article class="st"><h3><i style="background:#{html.escape(s["color"])}"></i>{html.escape(w["title"])}</h3>'
                f'<p>{html.escape(w["text"])}</p>{say}</article>')

    def release(r, latest):
        day = date.fromisoformat(r['date'])
        when = (f'{day:%B} {day.day}, {day.year}' if lang == 'en' else
                f'{day.day} de {MONTHS[lang][day.month - 1]} de {day.year}' if lang == 'es' else f'{day.day} {MONTHS[lang][day.month - 1]} {day.year}')
        mark = f' <span class="now">{html.escape(t["latest"])}</span>' if latest else ''   # the space: read aloud it is «0.8.5 Latest»
        return (f'<section class="rel" id="v{html.escape(r["version"])}"><header><h2>{html.escape(r["version"])}{mark}</h2>'
                f'<time datetime="{day.isoformat()}">{when}</time></header>\n'
                f'<div class="sts">{"".join(story(s) for s in r["stories"])}</div></section>')

    coin = re.search(r'<symbol id="f-wait" viewBox="([^"]*)">(.*?)</symbol>', landing, re.S)
    links = ''.join(f'<link rel="alternate" hreflang="{l}" href="{SITE}{p}whats-new/">\n' for l, (p, _) in LANGS.items())
    page = page.replace('{{links}}', f'<link rel="canonical" href="{SITE}{path}whats-new/">\n{links}'
                        f'<link rel="alternate" hreflang="x-default" href="{SITE}whats-new/">')
    page = page.replace('{{lang}}', lang).replace('{{home}}', '/' + path).replace('{{url}}', f'{SITE}{path}whats-new/')
    pc = windows_version()
    page = page.replace('{{version}}', html.escape(version)).replace('{{win}}', 'Windows' + (f' ({html.escape(pc)})' if pc else ''))
    for k, v in t.items():
        page = page.replace('{{t.' + k + '}}', html.escape(v))
    page = page.replace('{{fonts}}', ''.join(re.findall(r'  @font-face \{[^}]*\}\n', landing)))
    page = page.replace('{{coin}}', f'<svg viewBox="{coin.group(1)}" aria-hidden="true">{coin.group(2)}</svg>')
    page = page.replace('{{releases}}', '\n'.join(release(r, i == 0) for i, r in enumerate(releases)))
    if lang == 'ru':   # a one-letter word is tied to the next one, as on the landing
        page = re.sub(r'(?<![\w-])([ВвИиСсКкАаОоУуЯя]) ', '\\1\u00a0', page)
    return page


def main():
    version, out = sys.argv[1], Path(sys.argv[2])
    here = Path(__file__).parent
    releases = json.loads((here.parent.parent / 'Sources/IhatecryptoApp/WhatsNew/whatsnew.json').read_text(encoding='utf-8'))
    words = json.loads((here / 'whatsnew.i18n.json').read_text(encoding='utf-8'))
    news = (here / 'whatsnew.html').read_text(encoding='utf-8')
    page = (here / 'index.html').read_text(encoding='utf-8').replace('{{version}}', version)
    mf = (here / 'manifesto.html').read_text(encoding='utf-8').replace('{{version}}', version)
    T = json.loads(re.search(r'<script type="application/json" id="i18n">\n(.*?)\n</script>', page, re.S).group(1))
    for lang, (path, _) in LANGS.items():
        mine = words.get(lang, {})
        if lang != 'en':   # said aloud, so a release doesn't go out with a change nobody translated without anyone knowing
            missing = sum(not translated(mine, s) for r in releases for s in r['stories'])
            if missing:
                print(f"What's new in {lang}: {missing} changes have no translation in whatsnew.i18n.json yet and stay in English", file=sys.stderr)
        for target, text in [(out / path / 'index.html', enclave(render(page, lang, T), T[lang]['se'])),
                             (out / path / 'manifesto' / 'index.html', manifesto(mf, lang)),
                             (out / path / 'whats-new' / 'index.html', whatsnew(news, page, releases, lang, mine, version))]:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding='utf-8')
            print(target)


if __name__ == '__main__':
    main()
