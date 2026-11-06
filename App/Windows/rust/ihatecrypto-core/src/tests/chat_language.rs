use super::strings;
use crate::chat_language::*;

#[test]
fn chat_language_follows_what_the_user_wrote() {
    let auto = ChatLanguage::Auto;
    assert!(auto.is_russian(&strings(&["выведи всё из Jupiter Lend"])));
    assert!(!auto.is_russian(&strings(&["withdraw all from Jupiter"])));
    // The Confirm button types «do it»: that doesn't switch a Russian conversation to English.
    assert!(auto.is_russian(&strings(&["продай весь NEAR", "do it"])));
    assert!(auto.is_russian(&strings(&["sell NEAR", "продай весь NEAR", "cancel"])));
    // Numbers or an address alone say nothing either.
    assert!(auto.is_russian(&strings(&["купи на", "2"])));
    assert!(!auto.is_russian(&[]));
    // A pick in Settings wins.
    assert!(ChatLanguage::Ru.is_russian(&strings(&["hello"])));
    assert!(!ChatLanguage::En.is_russian(&strings(&["привет"])));
    assert_eq!(auto.name(&strings(&["привет"])), "Russian");
}

#[test]
fn a_bare_amount_does_not_switch_the_language() {   // R9
    let auto = ChatLanguage::Auto;
    assert!(auto.is_russian(&strings(&["положи USDC в jupiter", "1 sol"])));
    assert!(auto.is_russian(&strings(&["положи USDC в jupiter", "10 usdc"])));
    assert!(auto.is_russian(&strings(&["купи BONK", "0.5 BONK"])));
    assert_eq!(auto.name(&strings(&["застейкай сол", "$5"])), "Russian");
    assert!(!auto.is_russian(&strings(&["deposit USDC to jupiter", "1 sol"])));
    assert!(!auto.is_russian(&strings(&["положи USDC в jupiter", "buy 5 SOL"])));
}

#[test]
fn spanish_is_read_and_confirms_like_the_others() {
    assert_eq!(ChatLanguage::Auto.name(&strings(&["compra sol por 3 dólares"])), "Spanish");
    assert_eq!(ChatLanguage::Auto.name(&strings(&["largo en bitcoin con apalancamiento 3"])), "Spanish");
    assert_eq!(ChatLanguage::Auto.name(&strings(&["long btc 3x"])), "English");
    assert_eq!(ChatLanguage::Auto.name(&strings(&["лонг эфира"])), "Russian");
    assert_eq!(ChatLanguage::En.name(&strings(&["compra sol"])), "English");
    for yes in ["sí", "Sí, hazlo", "dale", "si", "ok", "Ок", "vale!", "do it", "делай", "да"] { assert!(ChatWords::confirms(yes), "{yes}") }
    for not_yes in ["si quieres compra sol", "ok but lower the risk", "long eth"] { assert!(!ChatWords::confirms(not_yes), "{not_yes}") }
    // pending: ChatRouter.decode and ChatRouter.commandOnly (Normalize round).
    // A confirmation is the whole message: with a change in it, the model reads it and the card waits again.
    for yes in ["да, давай", "давай делай", "ок, погнали", "yes, do it now", "go ahead", "let's do it", "sí, hazlo ya", "De acuerdo!"] {
        assert!(ChatWords::confirms(yes), "{yes}");
    }
    for not_yes in ["давай на 30", "да, но без камино", "yes but 50", "go long eth", "sign it with 30 USDC", "да сделай 200", "давай лучше в морфо", "ok тогда 200"] {
        assert!(!ChatWords::confirms(not_yes), "{not_yes}");
    }
    for no in ["no", "No gracias", "cancela", "déjalo", "нет", "cancel"] { assert!(ChatWords::declines(no), "{no}") }
    for no in ["стоп, передумал", "не, передумал", "нет, отмени", "отмени это", "nevermind", "cancel that", "forget it, I changed my mind", "no, cancel it"] {
        assert!(ChatWords::declines(no), "{no}");
    }
    for not_no in ["не на арбитруме", "нет, лучше 150", "не надо в kamino", "no, make it 50", "stop at 2600", "cancel alerts", "давай"] {
        assert!(!ChatWords::declines(not_no), "{not_no}");
    }
}

#[test]
fn questions_refusals_and_brush_offs_are_for_the_model() {
    for text in ["ok?", "да?", "confirm?", "sure?", "let it go", "да ну", "да ну его", "да не", "no don't", "¿sí?"] {
        assert!(!ChatWords::confirms(text), "{text}");
    }
    for text in ["не отменяй", "don't cancel", "отменить?", "не отменяй, давай"] { assert!(!ChatWords::declines(text), "{text}") }
    for text in ["да", "давай", "do it", "yes please", "ok", "ну давай", "let's go"] { assert!(ChatWords::confirms(text), "{text}") }
    for text in ["нет", "не надо", "не, передумал", "cancel", "never mind"] { assert!(ChatWords::declines(text), "{text}") }
}

#[test]
fn a_change_in_words_never_signs_the_card() {   // C10
    for change in ["добавь ещё триста", "make it five hundred", "давай половину", "удвой", "double it", "add another hundred",
                   "сделай 800", "да, но на тысячу", "yes but two thousand", "давай поменьше", "давай сто"] {
        assert!(ChatWords::may_change(change), "{change}");
    }
    for yes in ["да", "давай", "подтверждаю, всё верно", "yes, go ahead", "да, сделай это", "make it so", "окей, подписывай",
                "верно, адрес правильный", "the address is correct, go", "посмотрел, всё ок", "стоит, делай", "sounds good"] {
        assert!(!ChatWords::may_change(yes), "{yes}");
    }
}

/// Only the Confirm button carries out a waiting move: a typed «yes» in any language is answered, never signed.
#[test]
fn typed_approval_never_carries_out_a_move() {
    for yes in ["да", "делай", "do it", "yes, do it", "ok", "sí, hazlo", "погнали", "confirm"] {
        assert!(ChatWords::typed_approval(yes, false, true), "{yes}");
        assert!(!ChatWords::typed_approval(yes, true, true), "{yes} pressed");
        assert!(!ChatWords::typed_approval(yes, false, false), "{yes} nothing waiting");
    }
    for other in ["да?", "давай на 30", "не делай", "cancel", "what's the fee"] { assert!(!ChatWords::typed_approval(other, false, true), "{other}") }
}

/// ↑ walks back through what was sent, ↓ forward and out to an empty field; a field with its own text keeps its arrows.
#[test]
fn up_and_down_walk_what_was_sent() {
    let history = SentHistory::list(&strings(&["buy SOL for 5 USDC", "do it", "do it", "show my P/L", "show my P/L", "да", "send 1 USDC to Mom"]));
    assert_eq!(history, ["buy SOL for 5 USDC", "show my P/L", "send 1 USDC to Mom"]);
    let (mut at, mut draft) = (None, String::new());
    let press = |up: bool, at: &mut Option<usize>, draft: &mut String| {
        let Some((a, d)) = SentHistory::step(&history, *at, draft, up) else { return false };
        (*at, *draft) = (a, d);
        true
    };
    assert!(press(true, &mut at, &mut draft) && draft == "send 1 USDC to Mom");
    assert!(press(true, &mut at, &mut draft) && draft == "show my P/L");
    assert!(press(true, &mut at, &mut draft) && draft == "buy SOL for 5 USDC");
    assert!(press(true, &mut at, &mut draft) && draft == "buy SOL for 5 USDC");   // the oldest stays
    assert!(press(false, &mut at, &mut draft) && draft == "show my P/L");
    assert!(press(false, &mut at, &mut draft) && draft == "send 1 USDC to Mom");
    assert!(press(false, &mut at, &mut draft) && draft.is_empty() && at.is_none());   // past the newest: an empty field
    assert!(!press(false, &mut at, &mut draft));                                       // ↓ with nothing brought back isn't history's
    // An edited message is the user's own text now: the arrows move the caret in it.
    assert!(press(true, &mut at, &mut draft) && draft == "send 1 USDC to Mom");
    draft = "send 2 USDC to Mom".into();
    assert!(!press(true, &mut at, &mut draft) && !press(false, &mut at, &mut draft));
    assert!(SentHistory::step(&[], None, "", true).is_some());   // nothing sent yet: handled, nothing changes
}

/// «$SOL» is a token once closed by a space or a mark; «$5» is dollars; what's sent reads «SOL».
#[test]
fn dollar_mentions_name_tokens() {
    let tickers = |t: &str| TokenMentions::find(t).into_iter().map(|(_, t)| t).collect::<Vec<_>>();
    assert_eq!(tickers("buy $SOL for 2 USDC"), ["SOL"]);
    assert_eq!(tickers("swap $sol, then $eth."), ["sol", "eth"]);
    assert!(tickers("buy $SO").is_empty());   // still being typed
    assert!(tickers("pay $5 now").is_empty());
    assert!(tickers("a$SOL ").is_empty());
    let text = "buy $SOL for $5 then sell $eth";
    assert_eq!(&text[TokenMentions::find(text)[0].0.clone()], "$SOL");
    assert_eq!(TokenMentions::plain(text), "buy SOL for $5 then sell eth");
    assert_eq!(TokenMentions::plain("купи $JUP на $3"), "купи JUP на $3");
    // A contract or a mint after «$» is the token too; what's sent is the address as written.
    let (evm, mint) = ("0x4200000000000000000000000000000000000006", "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN");
    assert_eq!(tickers(&format!("buy ${evm} on base")), [evm]);
    assert_eq!(tickers(&format!("buy ${mint}, now")), [mint]);
    assert_eq!(TokenMentions::addresses(&format!("swap $SOL for ${mint} ")), [mint]);
    assert_eq!(TokenMentions::plain(&format!("buy ${evm}")), format!("buy {evm}"));
    assert!(!TokenMentions::is_address("SOL") && TokenMentions::is_address(evm));
}

/// FormsMatrixTests' yesAndNoAreWholeMessages, with its seed: any mix of yes-words and fillers signs; the same with a
/// change, a number, a question or a «not» in it never does; any mix of no-words drops the card.
#[test]
fn yes_and_no_are_whole_messages() {
    struct Seeded(u64);
    impl Seeded {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn pick<T: Clone>(&mut self, a: &[T]) -> T { a[(self.next() % a.len() as u64) as usize].clone() }
        fn shuffle<T>(&mut self, a: &mut [T]) { for i in (1..a.len()).rev() { a.swap(i, (self.next() % (i as u64 + 1)) as usize) } }
    }
    let mut rng = Seeded(1004);
    let sorted = |a: &[&'static str], without: &[&str]| { let mut v: Vec<&str> = a.iter().copied().filter(|w| !without.contains(w)).collect(); v.sort(); v };
    let (yes, filler) = (sorted(ChatWords::YES, &[]), sorted(ChatWords::FILLER, &[]));
    let (no, no_filler) = (sorted(ChatWords::NO, &["never"]), sorted(ChatWords::NO_FILLER, &["не", "no"]));
    let changes = ["на 30", "30", "но меньше", "only half", "instead", "on arbitrum", "без kamino", "потом", "later", "wait", "подожди", "?", "не",
                   "don't", "but", "только", "половину", "ещё 300", "another 300", "to kamino", "a bit less", "хм", "maybe", "может", "двадцать",
                   "twice", "в aave", "сначала", "first", "tomorrow", "завтра", "x3", "50%", "or", "или", "not yet", "ещё нет", "right"];
    let mut wrong = Vec::new();
    for _ in 0..5_000 {
        let mut words: Vec<&str> = (0..rng.next() % 3 + 1).map(|_| rng.pick(&yes)).collect();
        words.extend((0..rng.next() % 3).map(|_| rng.pick(&filler)).collect::<Vec<_>>());
        rng.shuffle(&mut words);
        let text = words.join(rng.pick(&[" ", ", ", " "])) + rng.pick(&["", "!", ".", " 👍"]);
        let line = format!(" {} ", words.join(" "));
        if !["да ну", "ну да", "let it go"].iter().any(|p| line.contains(&format!(" {p} "))) && (!ChatWords::confirms(&text) || ChatWords::declines(&text)) {
            wrong.push(format!("a yes not taken: «{text}»"));
        }
        let at = (rng.next() % (words.len() as u64 + 1)) as usize;
        words.insert(at, rng.pick(&changes));
        let changed = words.join(" ");
        if ChatWords::confirms(&changed) { wrong.push(format!("signs a change: «{changed}»")) }
        let mut nos: Vec<&str> = (0..rng.next() % 2 + 1).map(|_| rng.pick(&no)).collect();
        nos.extend((0..rng.next() % 3).map(|_| rng.pick(&no_filler)).collect::<Vec<_>>());
        rng.shuffle(&mut nos);
        let refusal = nos.join(rng.pick(&[" ", ", "])) + rng.pick(&["", "!", "."]);
        if !ChatWords::declines(&refusal) || ChatWords::confirms(&refusal) { wrong.push(format!("a no not taken: «{refusal}»")) }
    }
    for sarcasm in ["yeah right", "ну да, конечно", "да ну его"] { if ChatWords::confirms(sarcasm) { wrong.push(format!("signs «{sarcasm}»")) } }
    assert!(wrong.is_empty(), "yes/no wrong {}: {:?}", wrong.len(), &wrong[..wrong.len().min(15)]);
}
