import EVMChain
import IhatecryptoCore
import SwiftUI
import TradeExecution

/// «Show approvals»: who may still spend which token, on Base, Arbitrum, Ethereum, Polygon and Robinhood — the unlimited
/// ones first — each with Revoke, which drafts the revoke's card (Confirm, one transaction, nothing spent).
struct ApprovalsBoard: View {
    @Environment(Workspace.self) private var ws
    @State private var list: [TokenApproval]?
    @State private var unread: [String] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: "checkmark.shield").font(.system(size: 22)).foregroundStyle(Brand.accent)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Token approvals").font(.ui(16, .bold))
                    Text(subtitle).font(.ui(10)).foregroundStyle(.secondary)
                }
                Spacer()
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.hoverIcon).help("Read them again")
                Button { withAnimation(StudioView.spring) { ws.detail = nil } } label: { Image(systemName: "xmark") }.buttonStyle(.hoverIcon)
            }
            if let list {
                if list.isEmpty {
                    Text("No token approvals left: nothing can spend your tokens but you.").font(.ui(12)).foregroundStyle(.secondary)
                }
                ForEach(list) { a in row(a) }
            } else {
                HStack(spacing: 8) { ProgressView().controlSize(.small); Text("Reading every network's approvals…").font(.ui(12)).foregroundStyle(.secondary) }
            }
            if !unread.isEmpty {
                Text("Couldn't read \(unread.map(WalletCommand.placeName).joined(separator: ", ")) now — try again in a minute.").font(.ui(10.5)).foregroundStyle(.secondary)
            }
        }
        .padding(16)
        .task(id: ws.approvalsChanged) { await load() }
    }

    private var subtitle: String {
        guard let list else { return "on Base, Arbitrum, Ethereum, Polygon, Robinhood" }
        let open = list.filter(\.unlimited).count
        return "\(list.count) active" + (open > 0 ? " · \(open) unlimited" : "") + " · ihatecrypto's own are exact and spent at once"
    }

    private func row(_ a: TokenApproval) -> some View {
        HStack(spacing: 10) {
            TokenLogo(name: ModuleNetwork(place: a.chain)?.logo ?? a.chain, size: 22)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(a.symbol) → \(a.spenderName ?? "\(a.spender.prefix(6))…\(a.spender.suffix(4))")").font(.ui(12.5, .semibold)).lineLimit(1)
                Text("\(WalletCommand.placeName(a.chain))\(a.permit2 ? " · through Permit2" : "") · \(a.shown)")
                    .font(.ui(10)).foregroundStyle(a.unlimited ? Brand.loss : .secondary).lineLimit(1)
            }
            Spacer(minLength: 6)
            Button("Revoke") {
                Task { await ws.send(WalletCommand.revokeApproval(a.chain, token: a.token, spender: a.spender, permit2: a.permit2).summary) }
            }
            .help("Sets it to zero: one \(WalletCommand.placeName(a.chain)) transaction, nothing spent")
        }
        .padding(9)
        .background(RoundedRectangle(cornerRadius: 12).strokeBorder(a.unlimited ? Brand.loss.opacity(0.6) : Brand.cardBorder))
        .help("\(a.spender) may spend \(a.unlimited ? "all of" : "up to this much") \(a.symbol) (\(a.token)) of yours on \(WalletCommand.placeName(a.chain))")
    }

    private func load() async {
        guard let wallet = ws.previewWallet else { list = []; return }
        list = nil
        let read = await ws.executor(wallet).approvals()
        list = read.list
        unread = read.unread
    }
}
