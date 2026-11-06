//! «chart ETH week», «what's moving» (Charts.swift): the shared ChartCommand, drawn on the canvas — a chart of one
//! market, one a canvas, or the day's biggest movers.

use ihatecrypto_core::charts::ChartCommand;
use ihatecrypto_core::foundation::decimal;
use ihatecrypto_core::intent::{Fmt, IntentParser};
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::plan::Market;
use rust_decimal::Decimal;

use crate::blocks::{BlockData, Mover, MoversCard};
use crate::canvas::Board;
use crate::engine::{ChartLine, Engine, Message};

impl Engine {
    /// `quiet`: the movers' board only, without its list in the chat (the model's answer that opened it says it). Swift's
    /// default `quiet` is false.
    pub async fn chart(&self, command: ChartCommand, note: Option<String>, quiet: bool) {
        match command {
            ChartCommand::Movers => {
                if self.st().markets.is_empty() {
                    let read = Hyperliquid::markets().await.unwrap_or_default();
                    self.st_mut().markets = read;
                }
                let mut moving: Vec<Market> = self.st().markets.values().filter(|m| m.change24h().is_some() && m.volume24h > Decimal::from(1_000_000)).cloned().collect();
                moving.sort_by(|a, b| a.name.cmp(&b.name));
                moving.sort_by_key(|m| std::cmp::Reverse(m.change24h().unwrap_or_default()));
                if moving.is_empty() {
                    self.reply(&self.say("Markets aren't loaded yet.", "Рынки ещё не загрузились."), note);
                    return;
                }
                let line = |m: &Market| format!("{} {} {}", IntentParser::pretty(&m.name), Fmt::price(m.mark_px), self.signed(m.change24h().unwrap_or_default()));
                let mover = |m: &Market| Mover {
                    coin: m.name.clone(), name: IntentParser::pretty(&m.name).split(" (").next().unwrap_or(&m.name).into(), price: Fmt::price(m.mark_px),
                    change: m.change24h().unwrap_or_default(),
                };
                let up: Vec<&Market> = moving.iter().take(6).collect();
                let down: Vec<&Market> = moving.iter().rev().take(6).collect();
                let mut b = Board::new("result", &self.say("Movers · 24 h", "Движение · 24 ч"));
                b.lines = up.iter().map(|m| line(m)).chain(["—".to_string()]).chain(down.iter().map(|m| line(m))).collect();
                b.data = Some(BlockData { movers: Some(MoversCard { up: up.iter().map(|m| mover(m)).collect(), down: down.iter().map(|m| mover(m)).collect() }), ..Default::default() });
                b.request = self.last_request();
                self.pin(b);
                if quiet { return }
                let said = |list: &[&Market]| list.iter().map(|m| line(m)).collect::<Vec<_>>().join(", ");
                self.reply(&(self.say("Up the most today: ", "Сильнее всего выросли: ") + &said(&up) + &self.say(".\nDown the most: ", ".\nСильнее всего упали: ")
                             + &said(&down) + "."), Some(note.unwrap_or_else(|| "Hyperliquid".into())));
            }
            ChartCommand::Chart { coin, range } => match self.chart_line(&coin, range).await {
                Ok(line) => {
                    let price = |x: Option<&f64>| x.and_then(|x| decimal(&format!("{x}"))).map_or(String::new(), Fmt::price);
                    let (first, last) = (price(line.closes.first()), price(line.closes.last()));
                    let mut m = Message::new(false, &format!("{} · {}: {first} → {last} ({})", IntentParser::pretty(&coin), range.title(), line.change),
                                             Some(note.unwrap_or_else(|| "Hyperliquid".into())));
                    self.place_chart(line);
                    m.text += &self.say(" — the chart is on the canvas.", " — график на холсте.");
                    self.st_mut().messages_mut().push(m);
                    self.save_chat();
                    self.changed();
                }
                Err(e) => {
                    let pretty = IntentParser::pretty(&coin);
                    self.reply(&(self.say(&format!("No chart for {pretty} right now: "), &format!("Графика {pretty} сейчас нет: ")) + &e.0), note);
                }
            },
        }
    }

    /// A canvas has one chart, as the Mac's chart place: a new one — another coin or range — takes the place of the
    /// newest chart there, drawn anew and on top; charts drawn before stay as they were.
    pub fn place_chart(&self, line: ChartLine) -> String {
        let request = self.last_request();
        let old = self.st().boards().iter().find(|b| b.kind == "chart").map(|b| b.id.clone());
        if let Some(old) = old {
            self.update_board(Some(&old), |b| {
                b.title = line.title.clone();
                b.chart = Some(line);
                b.at = chrono::Utc::now();
                b.request = request;
            });
            self.set_boards(|boards| {
                if let Some(i) = boards.iter().position(|b| b.id == old) {
                    let b = boards.remove(i);
                    boards.insert(0, b);
                }
            });
            return old;
        }
        let mut b = Board::new("chart", &line.title);
        b.chart = Some(line);
        b.request = request;
        self.pin(b)
    }

    pub fn signed(&self, pct: Decimal) -> String { format!("{}{}%", if pct >= Decimal::ZERO { "+" } else { "" }, Fmt::num(pct, 2)) }
}

#[cfg(test)]
#[path = "charts/tests.rs"]
mod tests;
