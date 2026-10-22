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
