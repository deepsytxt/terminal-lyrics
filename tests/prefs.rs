//! The display keys save their choice to the config file, leaving the rest of
//! the user's file exactly as it was.

use std::path::PathBuf;

use terminal_lyrics::config::{ConfigFile, Sweep};
use terminal_lyrics::prefs::Prefs;
use terminal_lyrics::render::Case;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "terminal-lyrics-prefs-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("config.toml")
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_first_choice_creates_the_file() {
    let path = scratch("create");
    Prefs::new(Some(path.clone()))
        .set("font", "compact")
        .unwrap();

    let file = ConfigFile::parse(&read(&path)).unwrap();
    assert_eq!(file.font.as_deref(), Some("compact"));
}

#[test]
fn only_the_one_key_changes() {
    let path = scratch("in-place");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = "\
# my config

font   =   \"block\"   # the big one
case = \"upper\"

# colours
color_source = \"pywal\"
";
    std::fs::write(&path, before).unwrap();

    Prefs::new(Some(path.clone())).set("font", "mini").unwrap();
    assert_eq!(read(&path), before.replace("\"block\"", "\"mini\""));
}

#[test]
fn a_missing_key_is_added_and_the_rest_kept() {
    let path = scratch("append");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = "# keep me\nfont = \"block\"\n";
    std::fs::write(&path, before).unwrap();

    Prefs::new(Some(path.clone()))
        .set("visualizer", true)
        .unwrap();
    let after = read(&path);
    assert!(after.starts_with(before), "{after}");
    assert_eq!(ConfigFile::parse(&after).unwrap().visualizer, Some(true));
}

#[test]
fn a_file_that_is_not_toml_is_left_alone() {
    let path = scratch("broken");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = "font = \"block\nthis is not toml\n";
    std::fs::write(&path, before).unwrap();

    assert!(Prefs::new(Some(path.clone())).set("font", "mini").is_err());
    assert_eq!(read(&path), before);
}

#[cfg(unix)]
#[test]
fn a_linked_config_stays_linked() {
    let link = scratch("symlink");
    let dir = link.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    let real = dir.join("dotfiles-config.toml");
    std::fs::write(&real, "font = \"block\"\n").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    Prefs::new(Some(link.clone())).set("font", "mini").unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(read(&real), "font = \"mini\"\n");
}

#[test]
fn with_no_path_nothing_is_written() {
    Prefs::in_memory().set("font", "mini").unwrap();
}

/// Every label the keys save has to load back, or a saved choice would fail the
/// next start.
#[test]
fn every_saved_value_loads_back() {
    let path = scratch("round-trip");
    let prefs = Prefs::new(Some(path.clone()));

    for case in [Case::Upper, Case::Lower] {
        prefs.set("case", case.label()).unwrap();
        assert_eq!(ConfigFile::parse(&read(&path)).unwrap().case, Some(case));
    }
    for sweep in [Sweep::Auto, Sweep::Always, Sweep::Never] {
        prefs.set("sweep", sweep.label()).unwrap();
        assert_eq!(ConfigFile::parse(&read(&path)).unwrap().sweep, Some(sweep));
    }
    for on in [true, false] {
        prefs.set("word_by_word", on).unwrap();
        prefs.set("visualizer", on).unwrap();
        let file = ConfigFile::parse(&read(&path)).unwrap();
        assert_eq!((file.word_by_word, file.visualizer), (Some(on), Some(on)));
    }
    for font in terminal_lyrics::render::font::NAMES {
        prefs.set("font", *font).unwrap();
        let file = ConfigFile::parse(&read(&path)).unwrap();
        assert_eq!(file.font.as_deref(), Some(*font));
        assert!(terminal_lyrics::render::font::by_name(font).is_some());
    }
}
