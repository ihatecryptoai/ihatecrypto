//! The chat's language, the field's ↑/↓ history, «$SOL» mentions, and the short replies read without the model
//! (`ChatLanguage.swift`).

use std::ops::Range;

use crate::foundation::{all, all_chars, any_char, chars, count, group, has, is_letter, is_number, is_space, is_upper, split_chars, whole};

/// The language of ihatecrypto's own chat replies: the user's pick in Settings, or on «auto» the language of what
/// they last wrote themselves. The model is told the same language, so its replies match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatLanguage {
    Auto,
    En,
    Ru,
}

impl ChatLanguage {
    pub const ALL: [ChatLanguage; 3] = [Self::Auto, Self::En, Self::Ru];

    /// What the app types for the user (the Confirm and Cancel buttons) says nothing about their language.
    const APP_WORDS: [&str; 2] = ["do it", "cancel"];
    const UNITS: [&str; 15] = ["usdc", "usdt", "usdg", "usd", "sol", "eth", "btc", "jup", "near", "jitosol", "dollar", "dollars", "bucks", "k", "x"];

    /// «auto», «en», «ru»: the raw value Settings keeps.
    pub fn raw(self) -> &'static str { match self { Self::Auto => "auto", Self::En => "en", Self::Ru => "ru" } }

    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|l| l.raw() == raw) }

    pub fn is_russian(self, recent_user_messages: &[String]) -> bool {
        match self {
            Self::Ru => true,
            Self::En => false,
            Self::Auto => Self::own(recent_user_messages).is_some_and(|m| m.chars().any(|c| ('\u{400}'..='\u{4ff}').contains(&c))),
        }
    }

    /// The last message that tells the user's language: not the app's own button words, and not a bare amount
    /// («1 sol», «10 usdc», «0.5 BONK»), which reads the same in any language (a Russian chat answered «1 sol» went English).
    fn own(messages: &[String]) -> Option<&str> {
        messages.iter().map(String::as_str).rev().find(|m| {
            any_char(m, is_letter) && !Self::APP_WORDS.contains(&m.to_lowercase().trim())
                && !(any_char(m, is_number) && split_chars(m, |g| !is_letter(g)).iter().all(|w| {
                    Self::UNITS.contains(&w.to_lowercase().as_str()) || (count(w) <= 6 && all_chars(w, |g| g.is_ascii() && is_upper(g)))
                }))
        })
    }

    /// For the model's instructions: «Russian», «Spanish» or «English». Spanish only on «auto»: the app's own lines
    /// stay English, the model's replies follow the user.
    pub fn name(self, recent_user_messages: &[String]) -> &'static str {
        if self.is_russian(recent_user_messages) { return "Russian" }
        match Self::own(recent_user_messages) {
            Some(own) if self == Self::Auto && Self::looks_spanish(own) => "Spanish",
            _ => "English",
        }
    }

    /// «compra sol por 3 dólares», «¿cuánto gané?»: Spanish letters or words English doesn't use.
    pub fn looks_spanish(text: &str) -> bool {
        let q = text.to_lowercase();
        has(&q, "[ñ¿¡áéíóú]")
            || has(&q, r"\b(?:hola|quiero|compra|comprar|vende|vender|largo|corto|cuanto|dolares|por\s+favor|porfa|apalancamiento|riesgo|abre|abrir|cierra|cerrar|retira|deposita|mueve|gracias|precio|ganancias?|perdidas?|dame|haz|muestrame|avisame|cuando|reglas|semana|guardar|presta|pon|sube|baja|borra|cambialo|ahora|mis|tengo|dale|hazlo|vale|cancela)\b")
    }
}

/// ↑ and ↓ in the chat's field walk what was sent, as a terminal's history does: ↑ in an empty field or on a message
/// brought back goes one further back, ↓ one forward and past the newest to an empty field. Confirmations («да», «do
/// it», most of them the Confirm button's own) and a message the same as the one before it aren't kept: sending one
/// again does nothing useful.
pub struct SentHistory;

impl SentHistory {
    pub fn list(sent: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for text in sent.iter().filter(|t| !ChatWords::confirms(t)) {
            if out.last() != Some(text) { out.push(text.clone()) }
        }
        out
    }

    /// The field's text and the place in `history` after ↑ (`up`) or ↓; nil when the key isn't history's: the field
    /// holds text of its own, or ↓ with nothing brought back.
    pub fn step(history: &[String], at: Option<usize>, draft: &str, up: bool) -> Option<(Option<usize>, String)> {
        let current = at.filter(|&i| i < history.len() && history[i] == draft);
        if up {
            if !draft.is_empty() && current.is_none() { return None }
            let back = current.unwrap_or(history.len()).checked_sub(1);
            return Some(match back { Some(b) => (Some(b), history[b].clone()), None => (current, draft.to_string()) });
        }
        let current = current?;
        Some(if current + 1 < history.len() { (Some(current + 1), history[current + 1].clone()) } else { (None, String::new()) })
    }
}

/// «$SOL» in the chat's field names a token, and so does «$0x…» (a contract) or «$<mint>»: the field draws it as a
/// chip with the token's logo (a contract's from its market), and what's sent reads «SOL» or the address. «$5» stays
/// dollars: a mention starts with a letter or is an address, at the start of a word.
pub struct TokenMentions;

impl TokenMentions {
    /// The mentions a space or a mark has closed: «$SO» still being typed at the end stays text. Each one's byte range
    /// («$SOL») and token («SOL», or the contract or mint as written).
    pub fn find(text: &str) -> Vec<(Range<usize>, String)> {
        all(text, r#"\$(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{0,11})(?=[\s,.;:!?)»"'])"#)
            .iter()
            .filter_map(|m| {
                let range = m.get(0)?.range();
                Self::starts_word(text, range.start).then(|| (range, group(m, 1).to_string()))
            })
            .collect()
    }

    /// What's sent: every mention as its ticker or address, the last one too; dollars stay.
    pub fn plain(text: &str) -> String {
        let mut out = text.to_string();
        for m in all(text, r"\$(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{0,11})(?![A-Za-z0-9])").iter().rev() {
            let range = m.get(0).expect("a match").range();
            if Self::starts_word(text, range.start) { out.replace_range(range, group(m, 1)) }
        }
        out
    }

    /// A contract or a mint rather than a ticker.
    pub fn is_address(token: &str) -> bool { whole(token, "0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}").is_some() }

    /// The contracts and mints mentioned, to look up.
    pub fn addresses(text: &str) -> Vec<String> { Self::find(text).into_iter().map(|(_, t)| t).filter(|t| Self::is_address(t)).collect() }

    fn starts_word(text: &str, i: usize) -> bool {
        chars(&text[..i]).last().is_none_or(|before| is_space(before) || ["(", "«", "\"", "'"].contains(before))
    }
}

/// Short replies read without the model: a «yes» to a card waiting for Confirm, a «no» that drops it.
pub struct ChatWords;

impl ChatWords {
    pub const YES: &[&str] = &[
        "делай", "делаем", "да", "давай", "подтверждаю", "подтверди", "раскладывай", "го", "гоу", "погнали", "поехали", "жми",
        "подписывай", "подписываю", "запускай", "ок", "окей", "ага", "угу", "согласен", "согласна",
        "do", "go", "yes", "confirm", "confirmed", "sign", "ok", "okay", "yep", "yeah", "sure", "proceed",
        "sí", "si", "dale", "hazlo", "confirmo", "adelante", "vale", "claro", "acuerdo",
    ];
    pub const NEGATIONS: &[&str] = &["не", "нет", "ни", "no", "not", "never", "nope", "nah", "don't", "don’t", "dont"];
    /// Words that can go with a «yes» without changing it: «да, давай, сделай это», «yes please, do it now».
    pub const FILLER: &[&str] = &[
        "это", "его", "её", "ее", "сделай", "сейчас", "теперь", "пожалуйста", "плиз", "так", "ну", "и", "вперёд", "вперед", "конечно", "хорошо",
        "it", "this", "that", "now", "please", "pls", "ahead", "let's", "let’s", "lets", "let", "us", "and", "then", "just",
        "de", "por", "favor", "ya", "lo",
    ];
    pub const NO: &[&str] = &[
        "нет", "неа", "отмена", "отмени", "отменяй", "отменить", "стоп", "передумал", "передумала", "забей", "забудь", "cancel", "stop",
        "nevermind", "never", "forget", "nvm", "no", "nope", "abort", "cancela", "cancelar", "olvídalo", "olvidalo", "déjalo", "dejalo",
    ];
    pub const NO_FILLER: &[&str] = &[
        "не", "надо", "нужно", "ну", "всё", "все", "это", "его", "её", "ее", "пожалуйста", "я", "давай", "лучше", "пока", "так",
        "it", "that", "this", "please", "mind", "i", "changed", "my", "actually", "lo", "mejor", "no",
    ];

    /// The message's words: letters, digits and apostrophes, lower case.
    fn words(text: &str) -> Vec<String> {
        let q = text.to_lowercase();
        split_chars(&q, |g| !is_letter(g) && !is_number(g) && g != "'" && g != "’").into_iter().map(str::to_string).collect()
    }

    /// A confirmation is the whole message: «делай», «да, давай», «yes, do it», «sí, hazlo», «ok». Anything more is a
    /// change or a question the model reads: «давай на 30» signed the waiting 20 when a confirmation only had to start
    /// with «давай»; «si quieres…» is an «if».
    /// A «?» asks and a «не / don't / not» refuses («ok?», «да?», «да не»); «да ну (его)» and «let it go» brush the
    /// card off, and «ну да, конечно» or «yeah right» may be sarcasm: all of them are for the model.
    pub fn confirms(text: &str) -> bool {
        let words = Self::words(text);
        let line = format!(" {} ", words.join(" "));
        if text.contains(['?', '¿']) || words.iter().any(|w| Self::NEGATIONS.contains(&w.as_str())) || line.contains(" да ну ") || line.contains(" ну да ")
            || line.contains(" let it go ")
        {
            return false;
        }
        !words.is_empty() && words.iter().all(|w| Self::YES.contains(&w.as_str()) || Self::FILLER.contains(&w.as_str())) && words.iter().any(|w| Self::YES.contains(&w.as_str()))
    }

    /// A typed approval while a card waits for Confirm: it's answered with «press Confirm» and moves nothing. Only the
    /// Confirm or Sign button's own «do it» (`pressed`) carries a move out, so text from a site, a note or a pasted
    /// message can't agree for the user, whatever it says.
    pub fn typed_approval(text: &str, pressed: bool, waiting: bool) -> bool { !pressed && waiting && Self::confirms(text) }

    /// A whole-message «no»: «нет», «cancel», «no gracias», «déjalo», «стоп, передумал», «nevermind, forget it».
    /// Like a confirmation, only no-words and fillers: «не на арбитруме» is a change the model reads.
    /// «отменить?» asks; «не отменяй», «don't cancel» keep the card («не, передумал», with its comma, still drops it).
    pub fn declines(text: &str) -> bool {
        let q = text.to_lowercase();
        if q.contains(['?', '¿'])
            || all(&q, r"(?:^|[^\p{L}'’])(?:не|not|never|don['’]?t|dont)\s+([\p{L}'’]+)").iter().any(|m| Self::NO.contains(&group(m, 1)))
        {
            return false;
        }
        if whole(&q, r"\s*(?:no|nope|cancel|stop|нет|не\s+надо|отмена|отмени\w*|стоп|nein|abbrechen|non|annuler|cancelar|cancela|no\s+gracias|déjalo|dejalo|olvídalo|olvidalo|detente|para|não|nao|nie|anuluj)[.!\s]*").is_some() {
            return true;
        }
        let words = Self::words(text);
        !words.is_empty() && words.iter().all(|w| Self::NO.contains(&w.as_str()) || Self::NO_FILLER.contains(&w.as_str())) && words.iter().any(|w| Self::NO.contains(&w.as_str()))
    }

    /// A message that may change the card waiting for Confirm rather than approve it as it is: a number in digits or
    /// words, «half», «double», «добавь ещё», «поменьше». A model read «add another 300» as a yes once; whatever a
    /// model reads, such a message never signs. «сделай» and «make it» alone are a yes («сделай это»); with a change
    /// they carry its number or word.
    pub fn may_change(text: &str) -> bool {
        any_char(text, is_number) || has(&text.to_lowercase(),
            r"(?:^|[^\p{L}])(?:добав|ещё|еще|удво|вдвое|половин|полтин|четверт|треть|сотн|сотк|сотен|тысяч|косар|двест|трист|четырест|пятьсот|шестьсот|семьсот|восемьсот|девятьсот|миллион|побольше|поменьше|больше|меньше|увелич|уменьш|один|одну|два|две|три|четыре|пять|шесть|семь|восемь|девять|десять|двадцат|тридцат|сорок|пятьдесят|шестьдесят|семьдесят|восемьдесят|девяност|сто(?!\p{L})|(?:add|another|double|twice|triple|half|halve|quarter|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|grand|million|increase|decrease|bigger|smaller)(?!\p{L}))")
    }
}
