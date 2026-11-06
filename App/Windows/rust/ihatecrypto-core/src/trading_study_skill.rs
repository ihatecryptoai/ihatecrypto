//! The owner's trading study (`TradingStudySkill.swift`, 2026-10-16, «по трейдингу у меня есть вот этот скилл его бы
//! лучше имплементировать»): their prompt as written, then how it maps onto the app's tools (the `trades`, `journal`
//! and `study` lookups, the `journal` and `study` commands). The text is in trading_study.txt, as the Swift has it.

use crate::skills::Skill;

impl Skill {
    pub fn trading_study() -> Skill {
        Skill {
            id: "trading-study".into(),
            name: "Trading study".into(),
            about: "Studies how the user trades: interviews them, rebuilds their history from public addresses, finds what is \
                    edge and what is luck, keeps a journal and a playbook. Never trades."
                .into(),
            text: include_str!("trading_study.txt").into(),
            built_in: true,
        }
    }
}
