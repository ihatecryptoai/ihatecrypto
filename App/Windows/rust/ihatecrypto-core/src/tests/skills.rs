use crate::skills::Skill;

#[test]
fn a_skill_file_reads_its_name_and_purpose() {
    let md = Skill::parse("---\nname: Swing review\ndescription: \"Reviews swing trades\"\n---\n# Steps\nDo this.", "SKILL.md");
    assert!(md.name == "Swing review" && md.about == "Reviews swing trades" && md.id == "swing-review" && md.text == "# Steps\nDo this.");
    let txt = Skill::parse("You are my trading researcher.\n\nMore.", "codex_trading_study_prompt.txt");
    assert!(txt.name == "Codex trading study prompt" && txt.about == "You are my trading researcher." && !txt.built_in);
    assert!(Skill::trading_study().text.contains("## HOW THIS STUDY RUNS IN IHATECRYPTO") && Skill::trading_study().id == "trading-study");
    assert_eq!(Skill::built_ins(), [Skill::trading_study()]);
    // Kept as Skills.swift's Codable writes it.
    let json = serde_json::to_value(&md).unwrap();
    assert_eq!(json, serde_json::json!({"id": "swing-review", "name": "Swing review", "about": "Reviews swing trades", "text": "# Steps\nDo this.", "builtIn": false}));
}
