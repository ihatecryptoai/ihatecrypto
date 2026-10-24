//! The mic in the chat field: Windows' own speech recognizer, in the language of the keyboard layout that's on (the
//! Mac's rule: what you'd type in is what you'd say). Nothing is recorded or kept; the words go into the field and the
//! person sends them.

#[cfg(windows)]
pub fn listen() -> Result<String, String> {
    use windows::Globalization::Language;
    use windows::Media::SpeechRecognition::{SpeechRecognitionResultStatus, SpeechRecognizer};
    use windows::Win32::Globalization::LCIDToLocaleName;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    use windows::core::HSTRING;

    // The layout of the window in front (ours, the mic was just pressed): its low word is the language.
    let tag = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let lang = (GetKeyboardLayout(thread).0 as usize & 0xFFFF) as u32;
        let mut name = [0u16; 85];
        let n = LCIDToLocaleName(lang, Some(&mut name), 0);
        if n > 1 { String::from_utf16_lossy(&name[..n as usize - 1]) } else { String::new() }
    };
    let said = |e: windows::core::Error| match e.code().0 as u32 {
        0x80045509 => "Windows' speech recognition is off: turn on Settings › Privacy & security › Speech › Online speech recognition.".to_string(),
        0x80070005 => "ihatecrypto may not use the microphone: Settings › Privacy & security › Microphone › Let desktop apps access your microphone.".to_string(),
        _ => format!("Voice didn't work: {}", e.message()),
    };
    // That language if Windows has it installed, else the system's speech language.
    let recognizer = Language::CreateLanguage(&HSTRING::from(tag.as_str()))
        .and_then(|l| SpeechRecognizer::Create(&l))
        .or_else(|_| SpeechRecognizer::new())
        .map_err(said)?;
    let compiled = recognizer.CompileConstraintsAsync().and_then(|op| op.join()).map_err(said)?;
    if compiled.Status().map_err(said)? != SpeechRecognitionResultStatus::Success {
        return Err("Windows has no speech recognition for this language: add it in Settings › Time & language › Speech.".into());
    }
    // It listens until a pause.
    let result = recognizer.RecognizeAsync().and_then(|op| op.join()).map_err(said)?;
    let _ = recognizer.Close();
    match result.Status().map_err(said)? {
        SpeechRecognitionResultStatus::Success => Ok(result.Text().map_err(said)?.to_string()),
        SpeechRecognitionResultStatus::TimeoutExceeded | SpeechRecognitionResultStatus::UserCanceled => Ok(String::new()),
        SpeechRecognitionResultStatus::MicrophoneUnavailable => Err("No microphone is available.".into()),
        _ => Err("Windows couldn't make out the words; try again.".into()),
    }
}

#[cfg(not(windows))]
pub fn listen() -> Result<String, String> {
    Err("Voice uses Windows' speech recognizer: it works on Windows.".into())
}

/// The recognition under way: Cancel (false) and Done (true) end it (`stop`).
#[cfg(windows)]
static LIVE: std::sync::Mutex<Option<std::sync::mpsc::Sender<Ended>>> = std::sync::Mutex::new(None);

#[cfg(windows)]
enum Ended {
    /// Done (true: the words go in the field) or Cancel (false: nothing does).
    Stop(bool),
    /// Windows ended it by itself: the mic went, a long silence, an error.
    Completed(windows::Media::SpeechRecognition::SpeechRecognitionResultStatus),
}

/// The first line `heard` gets: the language being listened to, ahead of any words (the window's «· Russian ·»).
#[cfg(windows)]
const LANGUAGE_MARK: char = '\u{1}';

/// As `listen`, but across pauses until Done or Cancel, as the Mac's dictation keeps listening (Voice.swift): Windows'
/// continuous session, the settled phrases joined, each guess along the way handed to `heard` (the live transcript).
/// First it hands `heard` the language, after LANGUAGE_MARK.
#[cfg(windows)]
pub fn listen_live(heard: impl Fn(String) + Send + Sync + 'static) -> Result<String, String> {
    use std::sync::{Arc, Mutex};
    use windows::Foundation::TypedEventHandler;
    use windows::Globalization::Language;
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs, SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession,
        SpeechRecognitionHypothesisGeneratedEventArgs, SpeechRecognitionResultStatus, SpeechRecognizer,
    };
    use windows::Win32::Globalization::LCIDToLocaleName;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    use windows::core::{HSTRING, Ref};

    let tag = unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let lang = (GetKeyboardLayout(thread).0 as usize & 0xFFFF) as u32;
        let mut name = [0u16; 85];
        let n = LCIDToLocaleName(lang, Some(&mut name), 0);
        if n > 1 { String::from_utf16_lossy(&name[..n as usize - 1]) } else { String::new() }
    };
    let said = |e: windows::core::Error| match e.code().0 as u32 {
        0x80045509 => "Windows' speech recognition is off: turn on Settings › Privacy & security › Speech › Online speech recognition.".to_string(),
        0x80070005 => "ihatecrypto may not use the microphone: Settings › Privacy & security › Microphone › Let desktop apps access your microphone.".to_string(),
        _ => format!("Voice didn't work: {}", e.message()),
    };
    let recognizer = Language::CreateLanguage(&HSTRING::from(tag.as_str()))
        .and_then(|l| SpeechRecognizer::Create(&l))
        .or_else(|_| SpeechRecognizer::new())
        .map_err(said)?;
    let compiled = recognizer.CompileConstraintsAsync().and_then(|op| op.join()).map_err(said)?;
    if compiled.Status().map_err(said)? != SpeechRecognitionResultStatus::Success {
        return Err("Windows has no speech recognition for this language: add it in Settings › Time & language › Speech.".into());
    }
    let heard = Arc::new(heard);
    // The language the recognizer listens in, as Windows names it (it may fall back to the system's speech language).
    let language = recognizer.CurrentLanguage().and_then(|l| l.DisplayName()).map(|n| n.to_string()).unwrap_or(tag);
    heard(format!("{LANGUAGE_MARK}{language}"));

    // What's settled (each phrase after its pause) and the guess at the phrase being said now.
    let settled = Arc::new(Mutex::new(String::new()));
    let guess = Arc::new(Mutex::new(String::new()));
    let both = |settled: &Mutex<String>, guess: &Mutex<String>| {
        let (s, g) = (settled.lock().map(|x| x.clone()).unwrap_or_default(), guess.lock().map(|x| x.clone()).unwrap_or_default());
        [s, g].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")
    };
    let (send, ended) = std::sync::mpsc::channel::<Ended>();

    let (s1, g1, h1) = (settled.clone(), guess.clone(), heard.clone());
    let hypothesis = recognizer.HypothesisGenerated(&TypedEventHandler::new(move |_: Ref<SpeechRecognizer>, args: Ref<SpeechRecognitionHypothesisGeneratedEventArgs>| {
        if let Some(a) = &*args {
            if let Ok(text) = a.Hypothesis().and_then(|h| h.Text()) {
                if let Ok(mut g) = g1.lock() { *g = text.to_string(); }
                h1(both(&s1, &g1));
            }
        }
        Ok(())
    })).map_err(said)?;
    let session = recognizer.ContinuousRecognitionSession().map_err(said)?;
    let (s2, g2, h2) = (settled.clone(), guess.clone(), heard.clone());
    let result = session.ResultGenerated(&TypedEventHandler::new(move |_: Ref<SpeechContinuousRecognitionSession>, args: Ref<SpeechContinuousRecognitionResultGeneratedEventArgs>| {
        if let Some(a) = &*args {
            if let Ok(r) = a.Result() {
                if r.Status().ok() == Some(SpeechRecognitionResultStatus::Success) {
                    if let Ok(text) = r.Text() {
                        let text = text.to_string();
                        if !text.trim().is_empty() {
                            if let Ok(mut s) = s2.lock() { if !s.is_empty() { s.push(' '); } s.push_str(text.trim()); }
                        }
                    }
                }
            }
        }
        if let Ok(mut g) = g2.lock() { g.clear(); }
        h2(both(&s2, &g2));
        Ok(())
    })).map_err(said)?;
    let done_send = send.clone();
    let completed = session.Completed(&TypedEventHandler::new(move |_: Ref<SpeechContinuousRecognitionSession>, args: Ref<SpeechContinuousRecognitionCompletedEventArgs>| {
        let status = args.as_ref().and_then(|a| a.Status().ok()).unwrap_or(SpeechRecognitionResultStatus::Unknown);
        let _ = done_send.send(Ended::Completed(status));
        Ok(())
    })).map_err(said)?;

    if let Ok(mut live) = LIVE.lock() { *live = Some(send); }
    let started = session.StartAsync().and_then(|op| op.join());
    let end = match started {
        Ok(()) => ended.recv().unwrap_or(Ended::Stop(false)),
        Err(e) => { if let Ok(mut live) = LIVE.lock() { *live = None; } return Err(said(e)); }
    };
    if let Ok(mut live) = LIVE.lock() { *live = None; }
    let outcome = match end {
        // Done: the phrase being said is finished first, so its words count too.
        Ended::Stop(true) => {
            let _ = session.StopAsync().and_then(|op| op.join());
            Ok(both(&settled, &guess))
        }
        Ended::Stop(false) => { let _ = session.CancelAsync().and_then(|op| op.join()); Ok(String::new()) }
        // Windows ended it by itself: what was heard goes into the field, and an error is said only when nothing was
        // (the Mac's Voice: «Didn't catch that» with an empty transcript).
        Ended::Completed(status) => {
            let words = both(&settled, &guess);
            match status {
                _ if !words.is_empty() => Ok(words),
                SpeechRecognitionResultStatus::MicrophoneUnavailable => Err("No microphone is available.".into()),
                SpeechRecognitionResultStatus::Success | SpeechRecognitionResultStatus::TimeoutExceeded
                    | SpeechRecognitionResultStatus::UserCanceled | SpeechRecognitionResultStatus::PauseLimitExceeded => Ok(words),
                SpeechRecognitionResultStatus::NetworkFailure =>
                    Err("Windows' online speech recognition didn't answer: check the internet connection and try again.".into()),
                _ => Err("Windows couldn't make out the words; try again.".into()),
            }
        }
    };
    let _ = session.RemoveResultGenerated(result);
    let _ = session.RemoveCompleted(completed);
    let _ = recognizer.RemoveHypothesisGenerated(hypothesis);
    let _ = recognizer.Close();
    outcome
}

#[cfg(not(windows))]
pub fn listen_live(_heard: impl Fn(String) + Send + Sync + 'static) -> Result<String, String> {
    Err("Voice uses Windows' speech recognizer: it works on Windows.".into())
}

/// Cancel (`done` false) or Done (true) on the recognition under way.
pub fn stop(done: bool) {
    #[cfg(windows)]
    if let Ok(live) = LIVE.lock() {
        if let Some(send) = live.as_ref() { let _ = send.send(Ended::Stop(done)); }
    }
    #[cfg(not(windows))]
    let _ = done;
}
