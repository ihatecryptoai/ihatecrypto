import AVFoundation
import Carbon
import Observation
import Speech

/// Speaking instead of typing: the words land in the chat's field as they are recognized, and the person sends them
/// (nothing is ever sent by voice alone). Recognition stays on this Mac when the language's on-device model is there.
/// Apple's recognizer can't tell the language by itself, so it listens in the keyboard's: a Russian layout, Russian.
@Observable
final class VoiceInput {
    enum State: Equatable { case idle, listening, failed(String) }

    private(set) var state = State.idle
    /// What's been heard so far.
    private(set) var transcript = ""
    /// How much of `transcript` has stopped changing: the rest is the recognizer's current guess, shown grey.
    private(set) var settled = 0
    /// The phrases before the last pause, and the recognizer's text when the last one ended.
    private var take = Take()
    /// The language it listens in, in words: «Russian».
    private(set) var language = ""
    /// The last few loudness readings, 0…1, for the wave in the field.
    private(set) var levels: [Double] = Array(repeating: 0, count: 36)
    /// Whether this take is recognized on the Mac (otherwise Apple's servers hear it).
    private(set) var onDevice = true
    private(set) var startedAt = Date.now

    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?

    var listening: Bool { state == .listening }

    func start() async {
        guard state != .listening else { return }
        transcript = ""
        settled = 0
        take = Take()
        levels = Array(repeating: 0, count: levels.count)
        guard await Self.allowed() else {
            state = .failed("Allow the microphone and speech recognition for ihatecrypto in System Settings › Privacy & Security.")
            return
        }
        let keyboard = Self.keyboardLanguage()
        let locale = Self.recognizerLocale(for: keyboard, among: SFSpeechRecognizer.supportedLocales())
        guard let locale, let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
            state = .failed("Speech recognition in \(Self.name(keyboard)) isn't available: switch the keyboard to another language and try again.")
            return
        }
        language = Self.name(keyboard)
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        onDevice = recognizer.supportsOnDeviceRecognition
        request.requiresOnDeviceRecognition = onDevice
        request.addsPunctuation = true

        let input = engine.inputNode
        input.removeTap(onBus: 0)
        // A Mac with no input (a Mac mini without a mic, AirPods switching profile) gives a 0 Hz format, and a tap with it
        // throws an Objective-C exception that ends the app.
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            state = .failed("No microphone: connect one, or choose it in System Settings › Sound › Input, and try again.")
            return
        }
        let tap = Self.tap(request) { [weak self] level in
            Task { @MainActor in self?.push(level) }
        }
        if #available(macOS 27, *) {
            // A format the input won't take throws here; the older call below raises an exception that ends the app.
            do { try input.installAudioTap(onBus: 0, bufferSize: 1024, format: format) { tap(AVAudioPCMBuffer(copying: $0), $1) } } catch {
                state = .failed("The microphone didn't start: \(error.localizedDescription)")
                return
            }
        } else {
            input.installTap(onBus: 0, bufferSize: 1024, format: format, block: tap)
        }
        engine.prepare()
        do { try engine.start() } catch {
            input.removeTap(onBus: 0)
            state = .failed("The microphone didn't start: \(error.localizedDescription)")
            return
        }
        self.request = request
        task = recognizer.recognitionTask(with: request, resultHandler: Self.results { [weak self] text, final, pause, error in
            Task { @MainActor in self?.heard(text, final: final, pause: pause, error: error) }
        })
        startedAt = .now
        state = .listening
    }

    /// Stops listening and keeps what was heard; the last words come a moment after the audio ends.
    func finish() async {
        guard state == .listening else { return }
        request?.endAudio()
        stopEngine()
        for _ in 0..<30 where task != nil { try? await Task.sleep(for: .milliseconds(50)) }
        task?.cancel()
        task = nil
        state = .idle
    }

    /// Stops listening and drops what was heard.
    func cancel() {
        task?.cancel()
        stopEngine()
        transcript = ""
        take = Take()
        state = .idle
    }

    private func stopEngine() {
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        request = nil
    }

    private func heard(_ text: String?, final: Bool, pause: Bool, error: String?) {
        if let text {
            let whole = take.add(text, pause: pause)
            settled = final || pause ? whole.count : Self.settledLength(was: transcript, now: whole)
            transcript = whole
        }
        guard final else { return }
        task = nil
        // The recognizer stopped by itself (a minute's limit, silence, an error) while still listening.
        guard state == .listening, request != nil else { return }
        stopEngine()
        state = transcript.isEmpty ? error.map { .failed("Didn't catch that: \($0)") } ?? .idle : .idle
    }

    private func push(_ level: Double) {
        levels.removeFirst()
        levels.append(level)
    }

    /// A take of several phrases. After a pause the recognizer either starts the next phrase from nothing or goes on
    /// from its old text; either way what was said before stays.
    struct Take {
        /// The phrases before the last pause.
        var earlier = ""
        /// The recognizer's own text when the last phrase ended: a result that goes on from it repeats it.
        var closed = ""
        /// The current phrase.
        var phrase = ""

        /// The whole take after the recognizer's latest text; `pause`: that text ended a phrase.
        mutating func add(_ text: String, pause: Bool) -> String {
            var new = text
            if !closed.isEmpty, new.hasPrefix(closed) { new = String(new.dropFirst(closed.count)).trimmingCharacters(in: .whitespaces) }
            if VoiceInput.restarted(was: phrase, now: new) { earlier = whole; closed = "" }
            phrase = new
            let all = whole
            if pause { earlier = all; phrase = ""; closed = text }
            return all
        }

        var whole: String { [earlier, phrase].filter { !$0.isEmpty }.joined(separator: " ") }
    }

    /// The recognizer began again instead of revising: no word in common at the start and shorter, or under half as long.
    static func restarted(was: String, now: String) -> Bool {
        guard !was.isEmpty, !now.isEmpty else { return false }
        let first = was.split(separator: " ").first == now.split(separator: " ").first
        return (!first && now.count < was.count) || now.count * 2 < was.count
    }

    /// The words the new guess keeps from the last one, as a length of `now`: those stopped changing.
    static func settledLength(was: String, now: String) -> Int {
        let a = was.split(separator: " "), b = now.split(separator: " ")
        let same = zip(a, b).prefix { $0 == $1 }.count
        // The last common word may still be growing («про» → «процентов»): only words with one after them count.
        let words = same == b.count ? max(0, same - 1) : same
        return b.prefix(words).map(\.count).reduce(0, +) + words
    }

    /// The current keyboard layout's language: «ru» for Russian – PC, «en» for ABC or U.S.
    static func keyboardLanguage() -> String {
        guard let source = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
              let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceLanguages),
              let languages = Unmanaged<CFArray>.fromOpaque(raw).takeUnretainedValue() as? [String],
              let first = languages.first else { return "en" }
        return Locale(identifier: first).language.languageCode?.identifier ?? "en"
    }

    /// The recognizer's locale for a language: the one of its usual country (ru → ru-RU, en → en-US), else any of it.
    static func recognizerLocale(for language: String, among supported: Set<Locale>) -> Locale? {
        let usual = Locale(identifier: Locale.Language(identifier: language).maximalIdentifier).region?.identifier
        let same = supported.filter { $0.language.languageCode?.identifier == language }
        return same.first { $0.region?.identifier == usual } ?? same.min { $0.identifier < $1.identifier }
    }

    static func name(_ language: String) -> String {
        Locale(identifier: "en_US").localizedString(forLanguageCode: language) ?? language
    }

    // TCC answers on its own queue: a MainActor closure here (the target's default) traps the moment it's called.
    nonisolated private static func allowed() async -> Bool {
        let speech = await withCheckedContinuation { c in
            SFSpeechRecognizer.requestAuthorization { @Sendable status in c.resume(returning: status == .authorized) }
        }
        return speech ? await AVCaptureDevice.requestAccess(for: .audio) : false
    }

    /// The request and the time of the last level reading, touched only from the audio thread.
    nonisolated private final class Tap: @unchecked Sendable {
        let request: SFSpeechAudioBufferRecognitionRequest
        var last = Date.distantPast
        /// The loudest buffer since the last reading, so a short syllable between readings still shows.
        var peak: Float = 0
        init(_ request: SFSpeechAudioBufferRecognitionRequest) { self.request = request }
    }

    // The audio tap and the recognizer call back on their own threads: these closures must not be MainActor's.
    nonisolated private static func tap(_ request: SFSpeechAudioBufferRecognitionRequest,
                                        level: @escaping @Sendable (Double) -> Void) -> @Sendable (AVAudioPCMBuffer, AVAudioTime) -> Void {
        let state = Tap(request)
        return { buffer, _ in
            state.request.append(buffer)
            guard let samples = buffer.floatChannelData?[0] else { return }
            let n = Int(buffer.frameLength)
            var sum: Float = 0
            for i in 0..<n { sum += samples[i] * samples[i] }
            state.peak = max(state.peak, n > 0 ? sqrt(sum / Float(n)) : 0)
            // A reading every 0.1 s: the wave moves at a calm pace.
            guard Date.now.timeIntervalSince(state.last) > 0.1 else { return }
            state.last = .now
            let rms = state.peak
            state.peak = 0
            // Speech sits around −40…−10 dB; map that to 0…1.
            level(min(1, max(0, (20 * log10(Double(max(rms, 1e-6))) + 50) / 40)))
        }
    }

    /// Text, whether it's the last result, whether a phrase just ended at a pause (the metadata comes only then), error.
    nonisolated private static func results(_ deliver: @escaping @Sendable (String?, Bool, Bool, String?) -> Void)
        -> @Sendable (SFSpeechRecognitionResult?, Error?) -> Void {
        { result, error in
            deliver(result?.bestTranscription.formattedString, (result?.isFinal ?? false) || error != nil,
                    result?.speechRecognitionMetadata != nil, error?.localizedDescription)
        }
    }
}
