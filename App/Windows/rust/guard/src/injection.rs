//! Screening of text that came from outside (a pasted message, a web page, token metadata, an API reply) before any
//! model sees it (`Injection.swift`). Local rules run always; a classifier such as Jev can add problems but can never
//! clear one, and nothing here approves a transaction: the transaction guard still checks every signature on its own.

use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::Verdict;

const COMMAND_PATTERNS: [&str; 8] = [
    r"ignore\s+(all\s+|any\s+|the\s+)?(previous|prior|above|earlier)\s+(instructions|rules|messages|prompts)",
    r"disregard\s+(all\s+|the\s+|any\s+)?(previous|prior|above|earlier)",
    // A prompt named as a header or a tag that opens new orders; a question about it is not a command.
    r"(system|developer)\s+(prompt|message)\s*[:>\]]|[\[<]\s*/?\s*(system|developer)\s*[\]>]|(new|override|updated)\s+(system|developer)\s+(prompt|message)|developer\s+mode",
    r"(new\s+instructions|do\s+not\s+tell\s+the\s+user|don't\s+tell\s+the\s+user)",
    // «you are now» and «ты теперь» only with a new role or mode: the person's own wishes pass (measured 2026-10-27).
    r"\byou\s+are\s+now\b[^.!?\n]{0,40}?(agent|assistant|\bbot\b|\bai\b(?![\s-]*tokens?)|model|admin|developer|mode\b|\bdan\b|unrestricted|unfiltered|uncensored|jailbr|without\s+(limits|rules|checks|restrictions|filters|confirm))\w*",
    r"(игнорируй|забудь|отмени)\s+(все\s+|всё\s+|предыдущие\s+|прежние\s+)*(инструкции|правила|указания)",
    r"(системн\w+\s+(промпт\w*|сообщени\w+)\s*:|нов\w+\s+системн\w+\s+промпт|новые\s+инструкции|не\s+(говори|сообщай)\s+пользователю)",
    r"\bты\s+теперь\b[^.!?\n]{0,40}?(агент|ассистент|\bбот(ом)?\b|\bии\b(?![\s-]*токен)|модел|админ|разработчик|режим|свобод|\bdan\b|jailbr|джейлбр|без\s+(огранич|правил|провер|подтвержд|вопрос|фильтр|цензур)|не\s+обязан)\w*",
];
/// What a lookup brings back speaks to no one: «you are now…», «ты теперь…» there is said to the model reading it.
const NEW_ROLE: &str = r"\b(you\s+are\s+now|ты\s+теперь)\b";
const TRANSFER_VERBS: &str = r"(send|transfer|withdraw|bridge|deposit|approve|pay|отправ\w*|перевед\w*|перевест\w*|переведи|выведи|вывест\w*|скинь|пополн\w*|оплат\w*)";
const EVM_ADDRESS: &str = r"0x[0-9a-fA-F]{40}";
const SOLANA_ADDRESS: &str = r"[1-9A-HJ-NP-Za-km-z]{32,44}";

fn compile(p: &str) -> Regex { Regex::new(p).expect("a valid pattern") }

static COMMANDS: LazyLock<Vec<Regex>> = LazyLock::new(|| COMMAND_PATTERNS.iter().map(|p| compile(p)).collect());
static NEW_ROLE_RE: LazyLock<Regex> = LazyLock::new(|| compile(NEW_ROLE));
static TRANSFER_RE: LazyLock<Regex> = LazyLock::new(|| compile(TRANSFER_VERBS));
static EVM_RE: LazyLock<Regex> = LazyLock::new(|| compile(EVM_ADDRESS));
static SOLANA_RE: LazyLock<Regex> = LazyLock::new(|| compile(SOLANA_ADDRESS));
/// «send all 0x… on base to Bybit»: an address right after the verb (and the amount) is the token sent, when a name
/// after «to» receives it. Group 1 the token, group 2 the receiver.
static TOKEN_SLOT: LazyLock<Regex> = LazyLock::new(|| compile(&format!(
    r#"(?i)(?:send|transfer|отправ\w*|перевед\w*|перевести|скинь|кинь)\s+(?:(?:all|весь|всё|все|\d+(?:[.,]\d+)?)\s+)?(?:(?:the|my|мои|мой)\s+)?(?:tokens?\s+|токен\w*\s+)?({EVM_ADDRESS}|{SOLANA_ADDRESS})(?:\s+tokens?)?(?:\s+(?:on|in|в|на)\s+\S+)?\s+(?:to|на|для)\s+[«"“]?(\S+)"#)));

/// Every match, as written. A match the engine gives up on (its backtracking limit) ends the list.
fn all(re: &Regex, text: &str) -> Vec<String> {
    re.find_iter(text).map_while(Result::ok).map(|m| m.as_str().to_string()).collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    pub commands: Vec<String>,
    pub addresses: Vec<String>,
    pub transfer_verb: bool,
    /// Addresses that name the token sent, not who gets it.
    pub tokens: Vec<String>,
}

impl Findings {
    /// The addresses that would receive the money.
    pub fn recipients(&self) -> Vec<&String> { self.addresses.iter().filter(|a| !self.tokens.contains(a)).collect() }
}

pub fn findings(text: &str) -> Findings {
    let mut f = Findings::default();
    let lower = text.to_lowercase();
    for re in COMMANDS.iter() { f.commands.extend(all(re, &lower).into_iter().take(1)) }
    f.addresses = all(&EVM_RE, text);
    // A Solana address mixes digits, upper and lower case; plain words of that length do not.
    let evm = f.addresses.clone();
    f.addresses.extend(all(&SOLANA_RE, text).into_iter().filter(|s| {
        s.chars().any(|c| c.is_numeric()) && s.chars().any(char::is_uppercase) && s.chars().any(char::is_lowercase)
            && !evm.iter().any(|e| e.contains(s.as_str()))
    }));
    f.transfer_verb = !all(&TRANSFER_RE, &lower).is_empty();
    for caps in TOKEN_SLOT.captures_iter(text).map_while(Result::ok) {
        let (Some(token), Some(to)) = (caps.get(1), caps.get(2)) else { continue };
        if f.addresses.iter().any(|a| to.as_str().contains(a.as_str())) { continue }
        f.tokens.push(token.as_str().to_string());
    }
    f
}

/// A classifier's answer: the label ("data", "instructions" or "payment_redirect") and the confidence 0…1 — how likely
/// the text is plain data when the label is «data», how likely it isn't when the label is one of the other two.
pub type Classified<'a> = Pin<Box<dyn Future<Output = Result<(String, f64), String>> + Send + 'a>>;

pub trait TextClassifier: Send + Sync {
    fn name(&self) -> &str;
    fn classify<'a>(&'a self, text: &'a str, source: &'a str) -> Classified<'a>;
}

pub const THRESHOLD: f64 = 0.7;

/// Why a text was stopped, as data: the problems' English stays for logs, the chat says these in its own language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Commands to the assistant the local rules found, as written.
    Commands(Vec<String>),
    /// The address from the text the money would go to.
    SendsTo(String),
    /// The classifier, in percent: someone else's orders to the assistant.
    Steers(i64),
    /// The classifier, in percent: funds pushed to the text's own address.
    Redirects(i64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screen {
    pub verdict: Verdict,
    /// One for each problem.
    pub stops: Vec<Stop>,
}

impl Screen {
    pub fn ok(&self) -> bool { self.verdict.ok() }
}

/// `Int(confidence * 100)`: truncated, as Swift does.
fn percent(confidence: f64) -> i64 { (confidence * 100.0) as i64 }

pub async fn screen(text: &str, source: &str, classifier: Option<&dyn TextClassifier>) -> Screen {
    let mut s = Screen::default();
    let f = findings(text);
    if !f.commands.is_empty() {
        s.verdict.problems.push(format!("The text has commands for the assistant: “{}”.", f.commands.join("”, “")));
        s.stops.push(Stop::Commands(f.commands.clone()));
    }
    if !f.addresses.is_empty() {
        match f.recipients().first() {
            Some(to) if f.transfer_verb => {
                s.verdict.problems.push(format!("The text asks to send funds to {to}. Addresses from text never become recipients."));
                s.stops.push(Stop::SendsTo((*to).clone()));
            }
            _ if f.transfer_verb => s.verdict.checks.push("The address names the token to send. Only a saved name receives it.".into()),
            _ => s.verdict.checks.push("The text has an address, but it will not become a payment recipient.".into()),
        }
    }
    let Some(classifier) = classifier else {
        s.verdict.checks.push("Checked by local rules.".into());
        return s;
    };
    let name = classifier.name();
    match classifier.classify(text, source).await {
        Ok((label, confidence)) => {
            let p = percent(confidence);
            match label.as_str() {
                "data" => s.verdict.checks.push(format!("{name}: looks like ordinary data ({p}%).")),
                "instructions" if confidence >= THRESHOLD => {
                    s.verdict.problems.push(format!("{name}: the text tries to steer the assistant ({p}%)."));
                    s.stops.push(Stop::Steers(p));
                }
                "payment_redirect" if confidence >= THRESHOLD => {
                    s.verdict.problems.push(format!("{name}: the text tries to redirect funds to its own address ({p}%)."));
                    s.stops.push(Stop::Redirects(p));
                }
                _ => s.verdict.checks.push(format!("{name} is unsure: {label} ({p}%). Local rules decide.")),
            }
        }
        Err(error) => s.verdict.checks.push(format!("{name} is unavailable ({error}), so local rules apply.")),
    }
    s
}

/// What a lookup brought back for the chat's model — a token's listing, a page, a service's docs, a contract's string —
/// before the model reads it: None when it is plain data, else why it is withheld. The local rules look for commands to
/// the assistant only (an address beside «deposit» is what such texts are made of); the classifier adds what is said in
/// other words. Whatever is decided here, an address a lookup finds never becomes a recipient.
pub async fn lookup_problem(text: &str, source: &str, classifier: Option<&dyn TextClassifier>) -> Option<String> {
    let mut commands = findings(text).commands;
    commands.extend(all(&NEW_ROLE_RE, &text.to_lowercase()).into_iter().take(1));
    if !commands.is_empty() { return Some(format!("it has commands for the assistant: “{}”", commands.join("”, “"))) }
    let classifier = classifier?;
    let (label, confidence) = classifier.classify(text, source).await.ok()?;
    if label == "data" || confidence < THRESHOLD { return None }
    let (name, p) = (classifier.name().to_lowercase(), percent(confidence));
    Some(if label == "instructions" { format!("it tries to steer the assistant ({name}, {p}%)") }
         else { format!("it pushes funds to an address of its own ({name}, {p}%)") })
}
