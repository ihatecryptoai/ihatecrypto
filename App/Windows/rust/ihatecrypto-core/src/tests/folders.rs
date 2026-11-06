use super::strings;
use crate::folders::FolderCommand;

fn save(f: &str) -> Option<FolderCommand> { Some(FolderCommand::Save { folder: f.into() }) }

// Folders from the chat: saving the open canvas there, or writing a note into one.
#[test]
fn folder_commands_parse() {
    assert_eq!(FolderCommand::parse("save this canvas to folder Research"), save("Research"));
    assert_eq!(FolderCommand::parse("сохрани этот холст в папку Ресерч"), save("Ресерч"));
    assert_eq!(FolderCommand::parse("положи в папку «Токены на Robinhood»"), save("Токены на Robinhood"));
    assert_eq!(FolderCommand::parse("write to folder Ideas: buy MOO if it holds $0.006"),
               Some(FolderCommand::Note { folder: "Ideas".into(), text: "buy MOO if it holds $0.006".into() }));
    assert_eq!(FolderCommand::parse("запиши в папку Идеи: проверить SHROOM завтра\nи продать половину"),
               Some(FolderCommand::Note { folder: "Идеи".into(), text: "проверить SHROOM завтра\nи продать половину".into() }));
    assert_eq!(FolderCommand::parse("send 5 USDC to Mom"), None);
    assert_eq!(FolderCommand::parse("save 0x060b0fB0Be9d90557577B3AEE480711067149Ff0 as Shop on tempo"), None);
    // pending: ChatRouter.parses (Normalize round).
}

/// A typo of a folder that's there means that folder (route-check 2026-10-07: «Resarch» made a new one beside «Research»).
#[test]
fn a_folder_typo_means_the_folder_there() {
    let folders = strings(&["Research", "Ресерч", "ideas", "UITest"]);
    let existing = |n: &str| FolderCommand::existing(n, &folders);
    assert_eq!(existing("Resarch").as_deref(), Some("Research"));
    assert_eq!(existing("Reesarch").as_deref(), Some("Research"));
    assert_eq!(existing("research").as_deref(), Some("Research"));
    assert_eq!(existing("Ресеррч").as_deref(), Some("Ресерч"));
    assert_eq!(existing("ideaz").as_deref(), Some("ideas"));
    // Two neighbours swapped is one slip (route-check 2026-10-08: «реесрч»).
    assert_eq!(existing("реесрч").as_deref(), Some("Ресерч"));
    assert_eq!(existing("Reserach").as_deref(), Some("Research"));
    // Short or far names stay new folders: «idea» → no guess under 4 letters off the mark, «Travel» is new.
    assert_eq!(existing("Travel"), None);
    assert_eq!(existing("ui"), None);
}
