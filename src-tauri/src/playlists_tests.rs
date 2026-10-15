use super::*;

const ID: &str = "37i9dQZF1DXcBWIGoYBM5M";

fn saved(name: &str) -> Saved {
    Saved { name: name.into(), kind: "playlist".into(), id: ID.into() }
}

#[test]
fn share_links_and_uris_are_read_from_a_sentence() {
    let want = Some(("playlist".to_string(), ID.to_string()));
    assert_eq!(find_link(&format!("https://open.spotify.com/playlist/{ID}?si=abc123def")), want);
    assert_eq!(find_link(&format!("my gaming playlist is <https://open.spotify.com/intl-de/playlist/{ID}?si=x>.")), want);
    assert_eq!(find_link(&format!("spotify:playlist:{ID}")), want);
    assert_eq!(find_link(&format!("https://open.spotify.com/album/{ID}")), Some(("album".to_string(), ID.to_string())));
    assert_eq!(find_link("https://open.spotify.com/user/someone"), None);
    assert_eq!(find_link("https://example.com/playlist/37i9dQZF1DXcBWIGoYBM5M"), None);
    assert_eq!(find_link("my gaming playlist"), None);
}

#[test]
fn teaching_a_name() {
    let link = format!("https://open.spotify.com/playlist/{ID}?si=abc");
    let want = Some(Ask::Save(saved("gaming")));
    assert_eq!(ask(&format!("my gaming playlist is {link}")), want);
    assert_eq!(ask(&format!("save {link} as my gaming playlist")), want);
    assert_eq!(ask(&format!("Hey, BYTE, remember my gaming playlist is {link}")), want);
    assert_eq!(ask(&format!("add {link} as gaming")), want);
    // A link with nothing to call it by is not a request.
    assert_eq!(ask(&link), None);
}

#[test]
fn playing_a_playlist_said_every_way() {
    let play = |n: &str| Some(Ask::Play { name: n.into() });
    assert_eq!(ask("open spotify to my gaming playlist"), play("gaming"));
    assert_eq!(ask("Hey, BYTE, open Spotify to my playlist road trip"), play("road trip"));
    assert_eq!(ask("play my workout playlist"), play("workout"));
    assert_eq!(ask("put on my chill playlist please"), play("chill"));
    assert_eq!(ask("start the late night coding playlist on spotify"), play("late night coding"));
    assert_eq!(ask("can you play my gaming playlist"), play("gaming"));
    assert_eq!(ask("shuffle my workout playlist"), play("workout"));
}

#[test]
fn listing_and_forgetting() {
    assert_eq!(ask("what playlists do you know"), Some(Ask::List));
    assert_eq!(ask("show my playlists"), Some(Ask::List));
    assert_eq!(ask("forget my gaming playlist"), Some(Ask::Forget { name: "gaming".into() }));
}

#[test]
fn other_sentences_are_left_alone() {
    for q in [
        "how do I make a playlist",
        "what is a playlist",
        "play some jazz",
        "play music",
        "create a playlist of songs for a road trip",
        "what's playing",
        "write a poem about my gaming playlist",
        "my gaming playlist is really good",
    ] {
        assert_eq!(ask(q), None, "{q}");
    }
}

#[test]
fn names_are_matched_loosely_and_unknown_ones_search() {
    let mut list = vec![saved("late night coding"), Saved { name: "workout".into(), kind: "playlist".into(), id: "ABCDEFGHIJKLMNOPQRSTUV".into() }];
    assert_eq!(find(&list, "my workout playlist").map(|s| s.name.as_str()), Some("workout"));
    assert_eq!(find(&list, "coding").map(|s| s.name.as_str()), Some("late night coding"));
    assert_eq!(find(&list, "gaming"), None);
    let d = apply(&mut list, Ask::Play { name: "workout".into() });
    assert_eq!(d.open.as_deref(), Some("spotify:playlist:ABCDEFGHIJKLMNOPQRSTUV:play"));
    let d = apply(&mut list, Ask::Play { name: "gaming music".into() });
    assert_eq!(d.open.as_deref(), Some("spotify:search:gaming%20music"));
    assert!(d.notes.contains("share link"), "{}", d.notes);
}

#[test]
fn saving_replaces_a_name_and_forgetting_removes_it() {
    let mut list = Vec::new();
    apply(&mut list, Ask::Save(saved("gaming")));
    let other = Saved { name: "my Gaming playlist".into(), kind: "playlist".into(), id: "ZZZZZZZZZZZZZZZZZZZZZZ".into() };
    apply(&mut list, Ask::Save(other.clone()));
    assert_eq!(list, vec![other], "the same name is replaced, not doubled");
    assert_eq!(apply(&mut list, Ask::Forget { name: "gaming".into() }).card, "Forgot \"gaming\"");
    assert!(list.is_empty());
    assert_eq!(apply(&mut list, Ask::Forget { name: "gaming".into() }).card, "Not found");
}

#[test]
fn the_list_survives_a_restart_and_a_damaged_file_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    assert!(load(&path).is_empty());
    let list = vec![saved("gaming")];
    save(&path, &list);
    assert_eq!(load(&path), list);
    std::fs::write(&path, "{{").unwrap();
    assert!(load(&path).is_empty());
}
