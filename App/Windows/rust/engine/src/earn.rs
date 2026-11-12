//! The rates, spreading money across them, rebalancing, Merkl's rewards, coins from networks without a wallet here,
//! token approvals, Jupiter's DCA list and 1Click's limit orders (Earn.swift, LimitOrdersBoard) — the Mac's, over the same Executor and DeFi modules. What moves
//! money is a card; Confirm runs it.

use std::collections::HashSet;
use std::time::Duration;

use defi::yields::{self, YieldOption};
use evm::protocols::merkl::{self, Reward};
use evm::protocols::{aave, morpho};
use ihatecrypto_core::foundation::{Num, Rounding, iso8601_fraction, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use protocols::recurring::Order;
use rust_decimal::Decimal;
use signer_api::SignerRequest;
use tokio::task::JoinSet;
use trade::limit_order::{LimitOrderEnd, LimitOrderView};
use crate::notices::Notice;
use trade::morpho::MorphoRates;
use wallet::WalletError;

use crate::blocks::{BlockData, LimitRow, RateRow, ReceiveCard};
use crate::canvas::{Action, Board, Step};
use crate::engine::{Engine, Pending};
use crate::from_core::RouterOption;
use crate::wallet_ops::address_network;

/// `String.split(separator: "\n")`: the lines, empty ones left out.
fn lines(text: &str) -> Vec<String> { text.split('\n').filter(|l| !l.is_empty()).map(str::to_string).collect() }

/// «5.42» of a rate.
fn apy(o: &YieldOption) -> String { Fmt::num(rounded(o.apy, 2, Rounding::Plain), 2) }

/// What Merkl owes `user` now on its chains (EVMChain's `Merkl.rewards`); a reward claimed in full isn't listed.
async fn merkl_rewards(user: &str) -> Result<Vec<Reward>, WalletError> {
    let mut out = vec![];
    for chain in merkl::CHAINS {
        let url = format!("https://api.merkl.xyz/v4/users/{user}/rewards?chainId={chain}");
        let unreached = |e: reqwest::Error| WalletError(if e.is_timeout() { "The request timed out.".into() } else if e.is_connect() { "Could not connect to the server.".into() } else { e.to_string() });
        let response = net::HTTP.get(&url).header("Accept", "application/json").timeout(Duration::from_secs(15)).send().await.map_err(unreached)?;
        let status = response.status().as_u16();
        let data = response.bytes().await.map_err(unreached)?;
        if status != 200 { return Err(WalletError(format!("Merkl answered {status} for chain {chain}."))) }
        out.extend(merkl::parse(&data, user)?);
    }
    Ok(out)
}

impl Engine {
    /// Where money earns now: the DeFi modules' rates with Aave's and Morpho's on every network.
    pub async fn load_yields(&self, asset: Option<&str>) -> Vec<YieldOption> {
        let mut options = yields::options(asset).await;
        if asset != Some("SOL") {
            let mut group = JoinSet::new();
            for m in aave::MARKETS {
                group.spawn_local(async move {
                    let apy = trade::aave::aave_verify(&m).await.ok()?;
                    let how = format!("lend 2 USDC on Aave{}", if m.chain == "base" { String::new() } else { format!(" on {}", m.chain_name) });
                    Some(YieldOption { asset: "USDC".into(), place: m.place(), apy, tvl_usd: None, how: Some(how), note: String::new() })
                });
            }
            for v in morpho::VAULTS {
                group.spawn_local(async move {
                    let apy = MorphoRates::net_apy(&v).await?;
                    let how = format!("deposit 2 USDC into Morpho{}", if v.chain == "base" { String::new() } else { format!(" on {}", v.chain_name) });
                    Some(YieldOption { asset: "USDC".into(), place: v.place(), apy, tvl_usd: None, how: Some(how), note: v.name.into() })
                });
            }
            while let Some(done) = group.join_next().await {
                if let Ok(Some(o)) = done { options.push(o) }
            }
        }
        options.sort_by_key(|o| std::cmp::Reverse(o.apy));
        options
    }

    pub async fn show_yields(&self, asset: Option<&str>, note: Option<String>) {
        let rates = self.load_yields(asset).await;
        if rates.is_empty() {
            self.reply(&self.say("The rate sources didn't answer right now.", "Источники ставок сейчас не ответили."), note);
            return;
        }
        let shown = rates.iter().take(10).map(|o| {
            format!("{} ({}): {}% ", o.place, o.asset, apy(o)) + &self.say("a year", "годовых")
                + &o.tvl_usd.map_or(String::new(), |t| {
                    let millions = rounded(t / Decimal::from(1_000_000), 1, Rounding::Plain);
                    format!(", {}M ", Fmt::usd(millions)) + &self.say("deposited", "в депозитах")
                })
        }).collect::<Vec<_>>().join("\n");
        self.reply(&(self.say("Where money earns now:\n", "Где сейчас платят больше всего:\n") + &shown), Some(note.unwrap_or_else(|| "DeFi".into())));
        self.set_options(rates.iter().take(4).filter_map(|o| o.how.as_ref().map(|how| RouterOption::new(&format!("{} · {}%", o.place, apy(o)), &o.note, how, false))).collect());
        let title = self.say("Rates", "Ставки") + &asset.map_or(String::new(), |a| format!(" · {a}"));
        let actions = rates.iter().take(4).filter_map(|o| o.how.as_ref().map(|how| Action { title: format!("{} · {}%", o.place, apy(o)), command: how.clone() })).collect();
        let data = BlockData {
            rates: Some(rates.iter().take(8).map(|o| RateRow { place: o.place.clone(), asset: o.asset.clone(), apy: o.apy, how: o.how.clone(), note: o.note.clone() }).collect()),
            ..BlockData::default()
        };
        self.pin_kind("result", &title, rates.iter().take(12).map(|o| format!("{} ({}) · {}%", o.place, o.asset, apy(o))).collect(), None, Some(actions), Some(data));
        self.changed();
    }

    /// «spread USDC across the best rates», «rebalance»: the plan first, as a card; the deposits only after Confirm.
    pub async fn prepare_plan(&self, command: &WalletCommand, note: Option<String>) {
        let e = match self.executor() {
            Ok(e) => e,
            Err(e) => return self.reply(&(self.say("Failed: ", "Не вышло: ") + &e.0), note),
        };
        let (text, ready, route): (String, bool, (Vec<Step>, Vec<String>));
        // With nothing to do: nothing free is said in words; a plan with nowhere to go, or a rebalance with nothing
        // worth moving, is drawn with its reasons (the Mac's allocationPreview and RebalanceBoard).
        let mut idle = None;
        match command {
            WalletCommand::Allocate { symbol, max_share, amount, without, from } => {
                let r = e.allocate(symbol, max_share.unwrap_or(Decimal::from(50)), *amount, without, from.as_deref(), false).await;
                let (said, r_ready, preview) = match r { Ok(r) => r, Err(e) => return self.reply(&(self.say("Failed: ", "Не вышло: ") + &e.to_string()), note) };
                (text, ready) = (said, r_ready && preview.free > Decimal::ZERO);
                route = self.allocation_steps(&preview);
                if !ready && preview.free > Decimal::ZERO {
                    idle = Some(self.say3("Nowhere to put it now — see the canvas.", "Сейчас некуда положить — смотри холст.", "Ahora no hay dónde ponerlo: mira el lienzo."));
                }
            }
            WalletCommand::Rebalance { max_share } => {
                let share = max_share.unwrap_or(Decimal::from(50));
                let (said, r_ready, plan) = match e.rebalance(share, false).await {
                    Ok(r) => r,
                    Err(e) => return self.reply(&(self.say("Failed: ", "Не вышло: ") + &e.to_string()), note),
                };
                (text, ready) = (said, r_ready);
                route = self.rebalance_steps(&plan, share);
                if !ready {
                    let nothing_deposited = plan.moves.is_empty() && plan.skipped.is_empty() && plan.notes.first().is_some_and(|n| n.starts_with("Nothing is deposited"));
                    idle = Some(if nothing_deposited {
                        self.say3("Nothing is deposited yet: «spread USDC across the best rates» puts free USDC to work.",
                                  "Депозитов пока нет: «разложи USDC по лучшим ставкам» пустит свободные USDC в работу.",
                                  "Aún no hay depósitos: «reparte USDC en las mejores tasas» pone a trabajar los USDC libres.")
                    } else {
                        self.say3("Nothing worth moving now — the reasons are on the canvas.", "Сейчас нечего перекладывать — причины на холсте.",
                                  "Ahora no vale la pena mover nada: los motivos están en el lienzo.")
                    });
                }
            }
            _ => return,
        }
        let note = Some(note.unwrap_or_else(|| "DeFi".into()));
        let (steps, notes) = route;
        if !ready {
            // A scheduled spread with nothing free says nothing (the Mac's quiet run).
            self.st_mut().quiet_check = true;
            match idle {
                Some(idle) => {
                    let mut board = Board::new("move", &command.summary());
                    board.lines = lines(&text).into_iter().chain(notes).collect();
                    board.steps = (!steps.is_empty()).then_some(steps);
                    board.state = Some("nothing".into());
                    self.pin(board);
                    self.reply(&idle, note);
                }
                None => self.reply(&text, note),
            }
            return;
        }
        self.show(Pending::Wallet(command.clone()), &command.summary(), lines(&text), vec![], vec![]);
        // Drawn as the Mac's route cards (blocks.rs): from each network, the bridges, into each place.
        if !steps.is_empty() {
            let card = self.st().card.as_ref().map(|c| c.id.clone());
            self.update_board(card.as_deref(), |b| { b.steps = Some(steps); b.lines.extend(notes) });
        }
        self.reply(&self.say3("The plan is on the card. Press Confirm to make it, or ask about it.", "План на карточке. Нажми «Подтвердить» или спроси о нём.",
                              "El plan está en la tarjeta. Pulsa Confirm para hacerlo o pregunta por él."), note);
    }

    pub async fn prepare_claim(&self, command: &WalletCommand, note: Option<String>) {
        let Some(me) = self.st().evm_address() else {
            return self.reply(&self.say("This wallet has no EVM address to claim rewards to.", "У этого кошелька нет EVM-адреса, куда забрать награды."), note);
        };
        let owed = match merkl_rewards(&me).await {
            Ok(owed) => owed,
            Err(e) => return self.reply(&(self.say("Couldn't ask Merkl: ", "Не достучался до Merkl: ") + &e.0), note),
        };
        if owed.is_empty() {
            return self.reply(&self.say("Nothing to claim: Merkl owes this wallet nothing on Base, Arbitrum, Ethereum or Polygon right now.",
                                        "Забирать нечего: Merkl ничего не должен этому кошельку ни на Base, ни на Arbitrum, Ethereum или Polygon."), note);
        }
        // Merkl's numbers are outside data: a sum past range is said as the most there is, never a panic.
        let worth = owed.iter().filter_map(Reward::worth_usd).fold(Num::of(Decimal::ZERO), |s, w| s + w).0.unwrap_or(Decimal::MAX);
        let shown = owed.iter().map(|r| format!("{} {}", Fmt::num(r.claimable(), 6), r.symbol)).collect::<Vec<_>>();
        if let WalletCommand::SweepRewards { min_usd, .. } = command && worth < merkl::sweep_floor(*min_usd) {
            let (list, floor) = (shown.join(", "), Fmt::usd(merkl::sweep_floor(*min_usd)));
            return self.reply(&self.say(&format!("Merkl owes {list}, ≈ {}: under the {floor} floor, so nothing is swept yet — the gas would eat it.", Fmt::usd(worth)),
                                        &format!("Merkl должен {list}, ≈ {}: меньше порога {floor}, поэтому пока ничего не забираю — газ съест.", Fmt::usd(worth))), note);
        }
        let mut card = shown.clone();
        if worth > Decimal::ZERO { card.push(format!("≈ {}", Fmt::usd(worth))) }
        self.show(Pending::Wallet(command.clone()), &command.summary(), card, vec![], vec![]);
        let list = shown.join(", ");
        self.reply(&self.say(&format!("Merkl owes this wallet {list}. Press Confirm to claim it."), &format!("Merkl должен кошельку {list}. Нажми «Подтвердить», чтобы забрать.")),
                   Some(note.unwrap_or_else(|| "Merkl".into())));
    }

    /// Coins from a network without a wallet here (Bitcoin, TRON, Sui…): NEAR Intents' deposit address, for an hour;
    /// what lands is USDC on Solana. Nothing is signed.
    pub async fn receive_across(&self, network: &str, token: &str, amount: Decimal, note: Option<String>) {
        let title = address_network(network).map_or_else(|| network.to_string(), |n| n.title().to_string());
        let account = match self.signer.send(&SignerRequest::NearAddress {}).await {
            Ok(r) => r.near_address,
            Err(e) => return self.reply(&e.0, note),
        };
        let Some(account) = account else { return self.reply("The signer has no NEAR account for a refund.", note) };
        let e = match self.executor() { Ok(e) => e, Err(e) => return self.reply(&e.0, note) };
        let (deposit, quote, minimum) = match e.receive_across(token, amount, network, &account).await {
            Ok(r) => r,
            Err(e) => return self.reply(&e.to_string(), note),
        };
        let (t, n, out) = (token.to_uppercase(), Fmt::num(amount, 2), quote.amount_out_formatted.clone());
        let card = ReceiveCard {
            network: network.into(), title: title.clone(), token: t.clone(), amount, address: deposit.clone(), minimum,
            until: quote.deadline.as_deref().and_then(iso8601_fraction), arrives: out.clone(), state: self.say("waiting for your deposit", "жду твой перевод"),
        };
        self.pin_kind("result", &self.say(&format!("Receive {t} from {title}"), &format!("Получить {t} из {title}")),
                      vec![self.say(&format!("Send {n} {t} on {title} within an hour to:"), &format!("Отправь {n} {t} в сети {title} в течение часа на:")), deposit.clone(),
                           self.say(&format!("≈ {out} USDC lands on Solana"), &format!("На Solana придёт ≈ {out} USDC"))],
                      None, None, Some(BlockData { receive: Some(card), ..BlockData::default() }));
        let first = self.st().boards().first().map(|b| b.id.clone());
        if let Some(id) = first {
            let (me, address) = (self.rc(), deposit.clone());
            tokio::task::spawn_local(async move { me.follow_across(&id, &address).await });
        }
        let min = Fmt::num(minimum, 2);
        self.reply(&self.say(&format!("Send {n} {t} on {title} within an hour to:\n{deposit}\n≈ {out} USDC lands on Solana (at least {min} {t})."),
                             &format!("Отправь {n} {t} в сети {title} в течение часа на:\n{deposit}\nНа Solana придёт ≈ {out} USDC (не меньше {min} {t}).")),
                   Some(note.unwrap_or_else(|| format!("NEAR Intents · {title}"))));
    }

    /// Who may still spend the wallet's tokens, on each network; each one can be revoked from its card.
    pub async fn show_approvals(&self, note: Option<String>) {
        let Ok(e) = self.executor() else { return };
        // Its board at once, «Reading every network's approvals…», filled when the read is back (blocks.rs).
        let board = self.reading_approvals();
        let (list, unread) = e.approvals().await;
        self.fill_approvals(&board, &list, &unread);
        if list.is_empty() {
            let not_read = if unread.is_empty() { String::new() } else { self.say(" Not read: ", " Не прочитано: ") + &unread.join(", ") + "." };
            return self.reply(&(self.say("No token approvals: nothing may spend this wallet's tokens.", "Разрешений нет: никто не может тратить токены этого кошелька.") + &not_read), note);
        }
        let shown = list.iter().take(20).map(|a| {
            format!("{} on {} → {}", a.symbol, a.chain, a.spender_name.as_deref().unwrap_or(&a.spender))
                + &if a.unlimited() { self.say(" · unlimited", " · без лимита") } else { String::new() } + if a.permit2 { " · Permit2" } else { "" }
        }).collect::<Vec<_>>().join("\n");
        self.reply(&(self.say("Token approvals:\n", "Разрешения на токены:\n") + &shown), Some(note.unwrap_or_else(|| self.say("approvals", "разрешения"))));
        self.set_options(list.iter().take(6).map(|a| {
            let c = WalletCommand::RevokeApproval { place: a.chain.clone(), token: a.token.clone(), spender: a.spender.clone(), permit2: a.permit2 };
            let who = a.spender_name.clone().unwrap_or_else(|| a.spender.chars().take(10).collect());
            RouterOption::new(&self.say(&format!("Revoke {} for {who}", a.symbol), &format!("Отозвать {} у {who}", a.symbol)),
                              &if a.unlimited() { self.say("unlimited", "без лимита") } else { String::new() }, &c.summary(), a.unlimited() && a.spender_name.is_none())
        }).collect());
        self.changed();
    }

    pub async fn show_jupiter_dca(&self, note: Option<String>) {
        let orders = match self.executor() { Ok(e) => e.jupiter_dca_orders().await.ok(), Err(_) => None };
        let Some(orders) = orders else { return self.reply(&self.say("Jupiter didn't answer.", "Jupiter не ответил."), note) };
        // Every board pinned before shows the orders as they are now; a new one only where none is (one live board).
        let here = self.read_dca(&orders);
        if !orders.is_empty() && !here { self.pin_dca(&orders) }
        let said = if orders.is_empty() { self.say("No Jupiter DCA is running.", "Jupiter DCA сейчас не запущен.") }
            else { self.say(&format!("Running Jupiter DCA: {}.", orders.len()), &format!("Запущено Jupiter DCA: {}.", orders.len())) };
        self.reply(&said, Some(note.unwrap_or_else(|| "Jupiter DCA".into())));
    }

    /// A new Jupiter DCA block on this canvas.
    fn pin_dca(&self, orders: &[Order]) {
        let data = BlockData { dca: Some(self.dca_rows(&orders[..orders.len().min(10)])), ..BlockData::default() };
        self.pin_kind("result", "Jupiter DCA", self.dca_lines(orders), None, None, Some(data));
    }

    /// The orders as read now: the session's board every canvas draws where none is pinned (boards.js), and every board
    /// pinned on any canvas shown as they are, as the Mac's one live board (Studio.swift:1379) — one pinned elsewhere
    /// never lists a cancelled order or old fills. True when this canvas has one pinned.
    pub fn read_dca(&self, orders: &[Order]) -> bool {
        let lines = if orders.is_empty() { vec![self.say("No Jupiter DCA is running.", "Jupiter DCA сейчас не запущен.")] } else { self.dca_lines(orders) };
        let data = (!orders.is_empty()).then(|| BlockData { dca: Some(self.dca_rows(&orders[..orders.len().min(10)])), ..BlockData::default() });
        let mut found = false;
        {
            let mut st = self.st_mut();
            st.jupiter_dca = (!orders.is_empty()).then(|| Board { id: "jupiter-dca".into(), lines: lines.clone(), data: data.clone(), ..Board::new("result", "Jupiter DCA") });
            let open = st.canvas_index();
            for (c, canvas) in st.canvases.iter_mut().enumerate() {
                for b in canvas.boards.iter_mut().filter(|b| b.kind == "result" && b.title == "Jupiter DCA") {
                    (b.lines, b.data) = (lines.clone(), data.clone());
                    if c == open { found = true }
                }
            }
        }
        self.save_canvases();
        self.changed();
        found
    }

    pub fn dca_lines(&self, orders: &[Order]) -> Vec<String> {
        let symbol = |mint: &str| -> String {
            if mint == SpotAsset::usdc().mint { return "USDC".into() }
            SpotAsset::all().iter().find(|a| a.mint == mint).map_or_else(|| mint.chars().take(6).collect::<String>() + "…", |a| a.symbol.clone())
        };
        orders.iter().take(10).map(|o| {
            format!("{} / {} {} → {} {} · ", Fmt::num(o.used, 2), Fmt::num(o.deposited, 2), symbol(&o.input_mint), Fmt::num(o.received, 6), symbol(&o.output_mint))
                + &self.say(&format!("{} swaps, every {} h", o.fills, o.every / 3600), &format!("{} обменов, раз в {} ч", o.fills, o.every / 3600))
        }).collect()
    }

    /// After a DCA is placed or cancelled the orders are read again (the Mac's Execution.swift:711-716): the Jupiter DCA
    /// block on the canvas shows them as they are now, so a cancelled order keeps no Cancel; a placed one with no block
    /// here gets one. Jupiter not answering leaves the block as it was.
    pub async fn refresh_jupiter_dca(&self, placed: bool) {
        let Ok(e) = self.executor() else { return };
        let Ok(orders) = e.jupiter_dca_orders().await else { return };
        let found = self.read_dca(&orders);
        if !found && placed && !orders.is_empty() { self.pin_dca(&orders) }
    }

    // MARK: Limit orders on 1Click

    /// This wallet's open limit orders (None: 1Click didn't answer). The NEAR account, which only the signer names, counts
    /// as one of its addresses.
    async fn own_limit_orders(&self) -> Option<Vec<LimitOrderView>> {
        let e = self.executor().ok()?;
        let known = self.st().near_address();
        let near = match known {
            Some(n) => Some(n),
            None => self.signer.send(&SignerRequest::NearAddress {}).await.ok().and_then(|r| r.near_address),
        };
        e.limit_orders(near.as_deref(), true).await.ok().map(|l| l.into_iter().map(|o| o.1).collect())
    }

    /// The 30 s loop's look at limit orders (the Mac's followLimitOrders). The wallet's orders are read once when the
    /// journal holds one not settled (at launch, after a wallet switch), so they're followed with no «show limit orders».
    /// Then an order that filled or expired leaves the board and is said in the chat and as a notification, with what came
    /// and its price after the fees. A canceled one isn't: the cancel said it. What left is written down in the journal as
    /// settled once its payout is final, asked at most 20 ticks.
    pub fn follow_limit_orders(&self) {
        if std::mem::replace(&mut self.st_mut().limit_following, true) { return }
        let me = self.rc();
        tokio::task::spawn_local(async move {
            me.follow_limits_once().await;
            me.st_mut().limit_following = false;
        });
    }

    async fn follow_limits_once(&self) {
        let Ok(e) = self.executor() else { return };
        let Some(wallet) = self.st().solana_address() else { return };
        let (views, followed) = { let st = self.st(); (st.limit_views.clone(), st.limit_followed.clone()) };
        let mine = views.as_ref().filter(|(w, _)| *w == wallet).map(|(_, v)| v.clone());
        // This wallet's board already shows (read by «show limit orders»): it's followed from here.
        if mine.is_some() && followed.as_deref() != Some(wallet.as_str()) { self.st_mut().limit_followed = Some(wallet.clone()) }
        else if followed.as_deref() != Some(wallet.as_str()) {
            // Another wallet's board stays only until this one's orders are read.
            if views.is_none() && !e.follows_limit_orders() { self.st_mut().limit_followed = Some(wallet); return }
            // Unread (1Click silent): asked again next tick.
            let Some(list) = self.own_limit_orders().await else { return };
            if self.st().solana_address().as_deref() != Some(wallet.as_str()) { return }
            self.st_mut().limit_followed = Some(wallet);
            self.read_limits(&list);
            return;
        }
        let payouts: HashSet<String> = self.st().limit_payouts.keys().cloned().collect();
        if !payouts.is_empty() {
            let settled = e.sync_limit_journal(Some(&payouts)).await;
            self.st_mut().limit_payouts.retain(|id, ticks| { *ticks = ticks.saturating_sub(1); !settled.contains(id) && *ticks > 0 });
        }
        let Some(shown) = mine.filter(|v| !v.is_empty()) else { return };
        let near = self.st().near_address();
        let Ok(now) = e.limit_orders(near.as_deref(), false).await else { return };
        let now: Vec<LimitOrderView> = now.into_iter().map(|o| o.1).collect();
        let mut left = vec![];
        for o in shown.iter().filter(|o| !now.iter().any(|n| n.id == o.id)) {
            // Unread: the board stays as it was and the next tick asks again, so a fill is never missed.
            let Some(end) = e.limit_order_end(&o.id).await else { return };
            left.push((o.clone(), end));
        }
        // Changed meanwhile (a cancel, a new order, another wallet): this read is the old one's.
        if self.st().limit_views.as_ref().is_none_or(|(w, v)| *w != wallet || *v != shown) { return }
        self.read_limits(&now);
        for (o, _) in &left { self.st_mut().limit_payouts.insert(o.id.clone(), 20); }
        for (o, end) in left.iter().filter(|(_, end)| end.status == "FILLED" || end.status == "EXPIRED") {
            let (en, ru) = limit_end_words(o, end);
            let body = self.say(&en, &ru);
            self.reply(&body, Some("NEAR Intents · limit order".into()));
            let title = if end.status == "FILLED" { self.say(&format!("Limit order filled: {}", o.coin), &format!("Лимитка исполнилась: {}", o.coin)) }
                else { self.say(&format!("Limit order expired: {}", o.coin), &format!("Лимитка истекла: {}", o.coin)) };
            self.notify(Notice::Orders, &title, &body, false, false, Some(false), Some(format!("limit:{}", o.coin)));
        }
    }

    /// «show limit orders»: the board of open orders, Cancel on each.
    pub async fn show_limit_orders(&self, note: Option<String>) {
        let Some(list) = self.own_limit_orders().await else { return self.reply(&self.say("1Click didn't answer.", "1Click не ответил."), note) };
        let here = self.read_limits(&list);
        if !list.is_empty() && !here { self.pin_kind("result", "Limit orders", self.limit_lines(&list), None, None, Some(self.limit_data(&list))) }
        let said = limit_list_words(&list).map_or_else(|| self.say("No open limit orders.", "Открытых лимитных ордеров нет."), |(en, ru)| self.say(&en, &ru));
        self.reply(&said, Some(note.unwrap_or_else(|| "NEAR Intents · limit orders".into())));
    }

    /// After an order is placed or canceled: the board shows the orders as they are now; a placed one with no board here
    /// gets one. 1Click not answering leaves the board as it was.
    pub async fn refresh_limit_orders(&self, placed: bool) {
        // A failed read here: the loop reads them again.
        let Some(list) = self.own_limit_orders().await else { self.st_mut().limit_followed = None; return };
        let found = self.read_limits(&list);
        if !found && placed && !list.is_empty() { self.pin_kind("result", "Limit orders", self.limit_lines(&list), None, None, Some(self.limit_data(&list))) }
    }

    /// The orders on every board pinned on any canvas, and the session's for the model. True when this canvas has one.
    fn read_limits(&self, list: &[LimitOrderView]) -> bool {
        let lines = if list.is_empty() { vec![self.say("No open limit orders.", "Открытых лимитных ордеров нет.")] } else { self.limit_lines(list) };
        let data = (!list.is_empty()).then(|| self.limit_data(list));
        let mut found = false;
        {
            let mut st = self.st_mut();
            st.limit_orders = (!list.is_empty()).then(|| Board { id: "limit-orders".into(), lines: lines.clone(), data: data.clone(), ..Board::new("result", "Limit orders") });
            st.limit_views = st.solana_address().map(|w| (w, list.to_vec()));
            let open = st.canvas_index();
            for (c, canvas) in st.canvases.iter_mut().enumerate() {
                for b in canvas.boards.iter_mut().filter(|b| b.kind == "result" && b.title == "Limit orders") {
                    (b.lines, b.data) = (lines.clone(), data.clone());
                    if c == open { found = true }
                }
            }
        }
        self.save_canvases();
        self.changed();
        found
    }

    fn limit_lines(&self, list: &[LimitOrderView]) -> Vec<String> {
        list.iter().map(|o| format!("{} {} {} at ${}, {}, filled {}{}", if o.sell { "sell" } else { "buy" }, Fmt::num(o.coins, 6), o.coin, Fmt::num(o.price, 2), o.state(),
                                    Fmt::num(o.filled, 6), o.market.map_or_else(String::new, |m| format!(", 1Click's price for it now ${}", Fmt::num(m, 2))))).collect()
    }

    fn limit_data(&self, list: &[LimitOrderView]) -> BlockData {
        let rows = list.iter().take(20).map(|o| LimitRow {
            id: o.id.clone(), sell: o.sell, coin: o.coin.clone(), coins: Fmt::num(o.coins, 6), price: Fmt::usd(o.price), filled: Fmt::num(o.filled, 6),
            done: if o.coins > Decimal::ZERO { (Num::of(o.filled) / o.coins).0.and_then(|x| x.to_string().parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 1.0) } else { 0.0 },
            status: o.status.clone(), state: o.state(), until: o.until.map(Fmt::date), cancel: format!("cancel limit orders id {}", o.id),
            market: o.market.filter(|m| *m > Decimal::ZERO).map(Fmt::usd),
            gap: o.market.filter(|m| *m > Decimal::ZERO).and_then(|m| (Num::of(o.price - m) / m * Decimal::ONE_HUNDRED).0).and_then(|g| g.to_string().parse::<f64>().ok()),
        }).collect();
        BlockData { limits: Some(rows), ..BlockData::default() }
    }
}

/// The open orders said in the chat: each one, not only their count. None for none.
pub(crate) fn limit_list_words(list: &[LimitOrderView]) -> Option<(String, String)> {
    if list.is_empty() { return None }
    let en = list.iter().map(|o| format!("{} {} {} at {}", if o.sell { "sell" } else { "buy" }, Fmt::num(o.coins, 6), o.coin, Fmt::usd(o.price))).collect::<Vec<_>>().join(" · ");
    let ru = list.iter().map(|o| format!("{} {} {} по {}", if o.sell { "продажа" } else { "покупка" }, Fmt::num(o.coins, 6), o.coin, Fmt::usd(o.price))).collect::<Vec<_>>().join(" · ");
    Some(if list.len() == 1 { (format!("One open limit order: {en}. It's on the canvas."), format!("Один открытый лимитный ордер: {ru}. Он на холсте."))
         } else { (format!("{} open limit orders: {en}. They're on the canvas.", list.len()), format!("Открытые лимитные ордера, {}: {ru}. Они на холсте.", list.len())) })
}

/// A filled or expired limit order in words: what went in and what comes out, at what price with every fee in.
pub(crate) fn limit_end_words(o: &LimitOrderView, end: &LimitOrderEnd) -> (String, String) {
    let (coin, limit) = (&o.coin, Fmt::usd(o.price));
    let cash = if o.sell { "USDC" } else { coin.as_str() };
    let order = format!("{} {} {coin} at {limit}", if o.sell { "sell" } else { "buy" }, Fmt::num(o.coins, 8));
    let order_ru = format!("{} {} {coin} по {limit}", if o.sell { "продажа" } else { "покупка" }, Fmt::num(o.coins, 8));
    let (mut en, mut ru) = (String::new(), String::new());
    if let Some(price) = end.price {
        let (gave, got) = (Fmt::num(end.spent, 8), Fmt::num(end.got, if o.sell { 2 } else { 6 }));
        en = if o.sell { format!("sold {gave} {coin} for ≈ {got} USDC") } else { format!("bought ≈ {got} {coin} for {gave} USDC") };
        ru = if o.sell { format!("продано {gave} {coin} за ≈ {got} USDC") } else { format!("куплено ≈ {got} {coin} за {gave} USDC") };
        en += &format!(", {} a {coin} with 1Click's fees and the network's (the order's price {limit})", Fmt::usd(price));
        ru += &format!(", {} за {coin} с комиссиями 1Click и сети (цена ордера {limit})", Fmt::usd(price));
    }
    if end.status == "FILLED" {
        return if en.is_empty() { (format!("Limit order filled: {order}. The {cash} comes to your own address."), format!("Лимитка исполнилась: {order_ru}. {cash} придут на твой адрес."))
               } else { (format!("Limit order filled: {en}. The {cash} comes to your own address."), format!("Лимитка исполнилась: {ru}. {cash} придут на твой адрес.")) };
    }
    (format!("Limit order expired: {order}.{} What didn't fill comes back to the address it came from.", if en.is_empty() { String::new() } else { format!(" Before that it {en}.") }),
     format!("Лимитка истекла: {order_ru}.{} Что не исполнилось, вернётся на адрес, откуда пришло.", if ru.is_empty() { String::new() } else { format!(" До этого {ru}.") }))
}

#[cfg(test)]
#[path = "earn/tests.rs"]
mod tests;
