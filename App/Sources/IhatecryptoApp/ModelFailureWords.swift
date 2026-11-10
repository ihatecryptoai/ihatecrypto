import Foundation
import LLMCore

extension Workspace {
    /// A model that didn't answer, in the person's words: what happened and what to do, a sentence each, in the chat's
    /// language — never the service's JSON (the owner, 2026-11-10). nil for a failure the chat has no words for: the
    /// service's own sentence is shown then. The service's own page (ProviderInfo.keyURL) is named where the fix is there.
    func modelFailureWords(_ failure: String, provider: ProviderInfo, model: String) -> (why: String, fix: String)? {
        guard var kind = LLMFailure.read(failure) else { return nil }
        // A program on this Mac says in its own words why it failed («model requires more system memory»): «overloaded,
        // send it again later» would never help. A subscription has no key: its 401 is a sign-in that ran out.
        if provider.kind == .local, kind == .busy { return nil }
        if provider.kind == .subscription, kind == .key { kind = .signIn }
        let p = provider.name
        let at = provider.keyURL.map { say(" at \($0.absoluteString)", " на \($0.absoluteString)", " en \($0.absoluteString)") } ?? ""
        let (why, fix): (String, String) = switch kind {
        case .credits:
            (say("\(p) says this account has no credits, or no spending limit is set for the key.",
                 "\(p) пишет, что на счёте нет денег или для ключа не задан лимит расходов.",
                 "\(p) dice que la cuenta no tiene saldo o que la clave no tiene límite de gasto."),
             say("Add credits or set the limit in your account\(at).", "Пополни баланс или задай лимит в кабинете\(at).",
                 "Añade saldo o fija el límite en tu cuenta\(at)."))
        case .planLimit:
            (say("The limit of your \(p) plan is used up until it resets.", "Лимит \(p) исчерпан, он обновится позже.",
                 "Se agotó el límite de tu plan de \(p) hasta que se renueve."),
             say("Wait for the reset.", "Подожди, пока он обновится.", "Espera a que se renueve."))
        case .rateLimit:
            (say("\(p) got too many requests and asks to wait.", "\(p) получил слишком много запросов и просит подождать.",
                 "\(p) recibió demasiadas solicitudes y pide esperar."),
             say("Send it again in a minute.", "Отправь ещё раз через минуту.", "Envíalo de nuevo en un minuto."))
        case .key:
            (say("\(p) didn’t take the key. It is wrong, revoked or expired.", "\(p) не принял ключ. Он неверный, отозван или истёк.",
                 "\(p) no aceptó la clave. Es incorrecta, fue revocada o caducó."),
             say("Make a new one\(at) and paste it in Settings › AI model.", "Создай новый\(at) и вставь в Settings › AI model.",
                 "Crea una nueva\(at) y pégala en Settings › AI model."))
        case .signIn:
            (say("The sign-in to \(p) has expired.", "Сессия \(p) истекла.", "La sesión de \(p) caducó."),
             say("Sign in again in Settings › AI model.", "Войди заново в Settings › AI model.", "Inicia sesión de nuevo en Settings › AI model."))
        case .access:
            (say("This key or account may not use \(model) at \(p).", "Этому ключу или аккаунту \(p) модель \(model) недоступна.",
                 "Esta clave o cuenta de \(p) no puede usar \(model)."), "")
        case .noModel:
            (say("\(p) has no model \(model).", "У \(p) нет модели \(model).", "\(p) no tiene el modelo \(model)."), "")
        case .tooLong:
            (say("This conversation is longer than \(model) can read.", "Разговор стал длиннее, чем \(model) может прочитать.",
                 "La conversación es más larga de lo que \(model) puede leer."),
             say("Start a new canvas.", "Начни новый холст.", "Empieza un lienzo nuevo."))
        case .busy:
            (say("\(p) is overloaded or down right now.", "\(p) сейчас перегружен или недоступен.", "\(p) está sobrecargado o caído ahora."),
             say("Send it again a little later.", "Отправь ещё раз чуть позже.", "Envíalo de nuevo un poco más tarde."))
        case .offline where provider.kind == .local:
            (say("\(p) isn’t running on this Mac.", "\(p) не запущен на этом Mac.", "\(p) no está abierto en este Mac."),
             say("Open it and send this again.", "Открой его и отправь ещё раз.", "Ábrelo y envíalo de nuevo."))
        case .offline:
            (say("There’s no connection to \(p) from this Mac.", "С этого Mac нет связи с \(p).", "No hay conexión con \(p) desde este Mac."),
             say("Check the internet and send it again.", "Проверь интернет и отправь ещё раз.", "Revisa internet y envíalo de nuevo."))
        case .timeout:
            (say("\(p) took too long to answer.", "\(p) отвечал слишком долго.", "\(p) tardó demasiado en responder."),
             say("Send it again.", "Отправь ещё раз.", "Envíalo de nuevo."))
        case .updating:
            (say("Claude Code is being updated on this Mac right now.", "Claude Code сейчас обновляется на этом Mac.",
                 "Claude Code se está actualizando en este Mac ahora."),
             say("Send it again in a minute or two.", "Отправь ещё раз через минуту-две.", "Envíalo de nuevo en un minuto o dos."))
        }
        let another = fix.isEmpty ? say("Pick another model above.", "Выбери другую модель сверху.", "Elige otro modelo arriba.")
                                  : fix + " " + say("Or pick another model above.", "Или выбери другую модель сверху.", "O elige otro modelo arriba.")
        return (why, another)
    }
}
