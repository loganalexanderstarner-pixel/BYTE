use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime};

use super::*;
use crate::engine::Endpoint;
use crate::macctl::RunError;

#[test]
fn upkeep_requests_are_recognized_and_others_are_not() {
    for q in ["What's taking up space on my Mac?", "free up space", "I'm running out of storage", "my disk is almost full", "find duplicate files", "show me my biggest files", "clean up my Mac please"] {
        assert_eq!(ask(q), Some(Ask::Storage), "{q}");
    }
    for q in ["why is my Mac so slow?", "my macbook is running hot", "what's draining my battery", "battery health", "what's using my memory", "how can I speed up my mac"] {
        assert_eq!(ask(q), Some(Ask::Coach), "{q}");
    }
    for q in ["check my Mac", "run a health check", "is my mac ok?"] {
        assert_eq!(ask(q), Some(Ask::Checkup), "{q}");
    }
    assert_eq!(ask("uninstall Zoom"), Some(Ask::Uninstall { app: "zoom".into() }));
    assert_eq!(ask("completely remove the Spotify app from my Mac"), Some(Ask::Uninstall { app: "spotify".into() }));
    assert_eq!(ask("uninstall “Microsoft Teams”"), Some(Ask::Uninstall { app: "microsoft teams".into() }));
    assert_eq!(ask("what opens at login?"), Some(Ask::LoginItems));
    assert_eq!(ask("show my login items"), Some(Ask::LoginItems));
    assert_eq!(ask("stop Spotify from opening at login"), Some(Ask::LoginRemove { name: "spotify".into() }));
    assert_eq!(ask("don't open Discord at startup"), Some(Ask::LoginRemove { name: "discord".into() }));
    assert_eq!(ask("remove Dropbox from login items"), Some(Ask::LoginRemove { name: "dropbox".into() }));
    for q in [
        "how much space does a 4K movie take",
        "what is a login item",
        "explain how storage works on a Mac",
        "my code is slow, how do I profile it",
        "is space infinite",
        "what does uninstall mean",
        "the battery in my car is dead",
        "remind me to check my mail at 5",
    ] {
        assert_eq!(ask(q), None, "{q}");
    }
}

#[test]
fn only_macs_with_it_switched_on() {
    assert!(!applies(false, "free up space"));
    assert_eq!(applies(true, "free up space"), cfg!(target_os = "macos"));
}

fn write(p: &Path, bytes: usize) -> PathBuf {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, vec![7u8; bytes]).unwrap();
    p.to_path_buf()
}

fn age(p: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(t).unwrap();
}

const SMALL: Limits = Limits { big: 40_000, dup_min: 10_000, old_days: 365, installer_days: 30 };

#[test]
#[cfg_attr(windows, ignore = "Upkeep is the Mac housekeeping feature: its fixtures, protection list and path handling are Mac-shaped. Not ported to Windows yet; it must be before the feature is offered there.")]
fn the_scan_sizes_folders_finds_copies_and_leaves_library_alone() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    write(&home.join("Movies/film.mov"), 60_000);
    let a = write(&home.join("Documents/report.pdf"), 20_000);
    let b = write(&home.join("Downloads/report copy.pdf"), 20_000);
    std::fs::write(&b, vec![7u8; 20_000]).unwrap();
    age(&b, 1);
    age(&a, 10);
    std::fs::write(home.join("Downloads/other.pdf"), vec![9u8; 20_000]).unwrap();
    let old = write(&home.join("Downloads/Zoom.pkg"), 5_000);
    age(&old, 90);
    let fresh = write(&home.join("Downloads/New.dmg"), 5_000);
    age(&fresh, 2);
    write(&home.join("Library/Mail/huge.mbox"), 90_000);
    write(&home.join("Library/Caches/com.app/cache.db"), 30_000);
    write(&home.join("Pictures/Photos Library.photoslibrary/originals/a.heic"), 60_000);
    write(&home.join(".npm/_cacache/content/x"), 30_000);
    write(&home.join(".config/thing"), 4_000);

    let s = scan(home, SMALL.dup_min, Duration::from_secs(10), 100_000);
    assert!(!s.partial);
    let names: Vec<&str> = s.folders.iter().map(|f| f.0.as_str()).collect();
    assert!(names.contains(&"Movies") && names.contains(&"Documents") && names.contains(&"Hidden folders"));
    assert!(!names.contains(&"Library"), "~/Library isn't walked");
    assert!(s.files.iter().all(|f| !f.path.starts_with(home.join("Library"))));
    assert!(s.app_caches >= 30_000);
    assert!(s.caches.iter().any(|(l, _, b)| l == "npm package cache" && *b >= 30_000));

    let dups = duplicates(&s.files, SMALL.dup_min, Instant::now() + Duration::from_secs(10));
    assert_eq!(dups.len(), 1, "same size but different contents isn't a copy");
    assert_eq!(dups[0][0].path, a, "the oldest copy is kept");
    assert_eq!(dups[0][1].path, b);

    let (card, allowed) = storage_card(home, &s, &dups, 1_000_000, 100_000, SMALL, SystemTime::now());
    let ids: Vec<&str> = card.suggestions.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"installers") && ids.contains(&"duplicates"), "{ids:?}");
    assert_eq!(allowed["installers"], vec![old.clone()], "only installers older than 30 days");
    assert_eq!(allowed["duplicates"], vec![b.clone()]);
    let big: Vec<&str> = card.big.iter().map(|b| b.path.as_str()).collect();
    assert_eq!(big, vec!["~/Movies/film.mov"], "files inside the Photos library are never offered");
    assert!(card.folders.iter().any(|f| f.name == "App caches"));
    // The npm cache is small here (under 50 MB), so it's shown but not suggested.
    assert!(!ids.iter().any(|i| i.starts_with("cache-")));
}

#[test]
fn offerable_files_skip_hidden_library_and_package_contents() {
    let h = Path::new("/Users/ada");
    assert!(offerable(h, Path::new("/Users/ada/Movies/a.mov")));
    assert!(offerable(h, Path::new("/Users/ada/Downloads/My.app")), "a whole app can go, not its insides");
    assert!(!offerable(h, Path::new("/Users/ada/Downloads/My.app/Contents/x")));
    assert!(!offerable(h, Path::new("/Users/ada/Library/Mail/x")));
    assert!(!offerable(h, Path::new("/Users/ada/.docker/disk.raw")));
    assert!(!offerable(h, Path::new("/Users/ada/Pictures/Photos Library.photoslibrary/originals/a.heic")));
    assert!(!offerable(h, Path::new("/Volumes/USB/a.mov")));
}

#[test]
#[cfg_attr(windows, ignore = "Upkeep is the Mac housekeeping feature: its fixtures, protection list and path handling are Mac-shaped. Not ported to Windows yet; it must be before the feature is offered there.")]
fn system_and_top_folders_are_never_trashed() {
    let h = Path::new("/Users/ada");
    for p in ["/System/Library/x", "/usr/bin/zsh", "/Applications", "/Applications/Utilities/Terminal.app", "/Users/ada", "/Users/ada/Documents", "/Users/ada/Library", "relative/path", "/Users/ada/Downloads/../Documents", "/"] {
        assert!(protected(h, Path::new(p)), "{p}");
    }
    for p in ["/Users/ada/Downloads/Zoom.pkg", "/Applications/Zoom.us.app", "/Users/ada/Library/Caches/com.x"] {
        assert!(!protected(h, Path::new(p)), "{p}");
    }
}

#[test]
fn trash_output_becomes_put_back_steps() {
    let paths = vec![PathBuf::from("/Users/ada/Downloads/a.dmg"), PathBuf::from("/Users/ada/Downloads/b.dmg"), PathBuf::from("/Users/ada/Downloads/c.dmg")];
    let out = "/Users/ada/.Trash/a.dmg\n!Finder got an error: Can't get file.\n/Users/ada/.Trash/c 2.dmg\n";
    let (back, bytes, failed) = put_back_args(&paths, &[10, 20, 30], out);
    assert_eq!(back, vec!["/Users/ada/.Trash/a.dmg", "/Users/ada/Downloads", "a.dmg", "/Users/ada/.Trash/c 2.dmg", "/Users/ada/Downloads", "c.dmg"]);
    assert_eq!(bytes, 40);
    assert_eq!(failed, vec!["b.dmg: Finder got an error: Can't get file."]);
}

#[tokio::test]
async fn trashing_goes_through_finder_with_paths_as_arguments() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let f = write(&home.join("Downloads/old; rm -rf ~.dmg"), 5_000);
    let keyed = Keyed::default();
    keyed.set("TRASH", Ok(format!("{}\n", home.join(".Trash/x.dmg").display())));
    let t = trash(&keyed, home, &[f.clone(), home.join("Documents")]).await;
    assert_eq!(t.moved, 1);
    assert!(t.undo.is_some() && t.error.is_none());
    let ran = keyed.ran.lock().unwrap().clone();
    let Command::Osa { script, args } = &ran[0] else { panic!() };
    assert_eq!(*script, TRASH);
    assert_eq!(args, &vec![f.display().to_string()], "one argument per file; ~/Documents itself is refused");
}

#[test]
fn scan_ids_let_each_list_be_trashed_once() {
    let mut m = HashMap::new();
    m.insert("installers".to_string(), vec![PathBuf::from("/Users/ada/Downloads/a.dmg")]);
    remember_scan("scan1", m);
    assert!(take_allowed("scan1", "duplicates").is_none());
    assert_eq!(take_allowed("scan1", "installers").unwrap().len(), 1);
    assert!(take_allowed("scan1", "installers").is_none(), "only once");
    assert!(take_allowed("nope", "installers").is_none());
}

#[test]
fn system_tool_output_is_read() {
    assert_eq!(parse_df("Filesystem 1024-blocks Used Available Capacity iused ifree %iused Mounted on\n/dev/disk3s5 482797652 390000000 92797652 81% 1 2 0% /System/Volumes/Data\n"), Some((482797652 * 1024, 92797652 * 1024)));
    assert_eq!(parse_df("garbage"), None);
    let batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=12345)\t85%; discharging; 5:12 remaining present: true\n";
    assert_eq!(parse_batt(batt), Some((85, true, "discharging".into(), Some("5:12".into()))));
    let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
    assert_eq!(parse_batt(ac), Some((100, false, "charged".into(), None)));
    assert_eq!(parse_batt("Now drawing from 'AC Power'\n"), None, "desktop Macs have no battery");
    let power = "Power:\n    Battery Information:\n      Health Information:\n          Cycle Count: 412\n          Condition: Normal\n          Maximum Capacity: 87%\n";
    assert_eq!(parse_power(power), (Some(412), Some("Normal".into()), Some(87)));
    assert_eq!(parse_mem_free("The system has 17179869184 (1048576 pages)\n...\nSystem-wide memory free percentage: 23%\n"), Some(23));
    let top = "Processes: 500 total\n%CPU MEM   COMMAND\n0.0  1G    WindowServer\n\nProcesses: 501 total\nLoad Avg: 3.1\n%CPU  MEM    COMMAND\n97.3  2104M+ Google Chrome He\n12.0  512M   WindowServer\n3.1   80M-   Music\n";
    assert_eq!(parse_top(top), vec![(97.3, "2104M".into(), "Google Chrome He".into()), (12.0, "512M".into(), "WindowServer".into()), (3.1, "80M".into(), "Music".into())]);
    let asserts = "Listed by owning process:\n   pid 318(coreaudiod): [0x1] 00:10:00 PreventUserIdleSleep named: \"x\"\n   pid 901(zoom.us): [0x2] 00:31:02 PreventUserIdleDisplaySleep named: \"call\"\n   pid 77(caffeinate): [0x3] 01:00:00 PreventUserIdleSystemSleep named: \"c\"\n   pid 901(zoom.us): [0x4] 00:31:02 PreventUserIdleSystemSleep named: \"call\"\n";
    assert_eq!(parse_awake(asserts), vec!["zoom.us", "caffeinate"]);
    assert_eq!(parse_boot("{ sec = 1727000000, usec = 12 } Sun Sep 22 10:13:20 2024"), Some(1727000000));
    assert_eq!(parse_backup("/Volumes/Backups/2026-09-29-101010.previous\n"), chrono::NaiveDate::from_ymd_opt(2026, 9, 29));
    assert_eq!(parse_backup("/Volumes/.timemachine/ABC/2026-09-28-080000.backup/2026-09-28-080000.backup\n"), chrono::NaiveDate::from_ymd_opt(2026, 9, 28));
    assert_eq!(parse_backup("No machine directory found for host."), None);
    assert_eq!(size_text(1_234_000_000), "1.2 GB");
    assert_eq!(size_text(52_000_000), "52 MB");
}

#[test]
fn busy_programs_match_their_apps() {
    let apps: Vec<String> = ["Google Chrome", "Music", "Microsoft Word", "Finder"].iter().map(|s| s.to_string()).collect();
    assert_eq!(app_for("Google Chrome He", &apps), Some("Google Chrome".into()), "top cuts names");
    assert_eq!(app_for("Music", &apps), Some("Music".into()));
    assert_eq!(app_for("Microsoft W", &apps), Some("Microsoft Word".into()));
    assert_eq!(app_for("WindowServer", &apps), None);
    assert_eq!(app_for("Mus", &apps), None, "too short to guess");
}

#[test]
#[cfg_attr(windows, ignore = "Upkeep is the Mac housekeeping feature: its fixtures, protection list and path handling are Mac-shaped. Not ported to Windows yet; it must be before the feature is offered there.")]
fn apps_are_found_and_their_leftovers_listed() {
    let d = tempfile::tempdir().unwrap();
    let apps = d.path().join("Applications");
    std::fs::create_dir_all(apps.join("zoom.us.app/Contents")).unwrap();
    std::fs::create_dir_all(apps.join("Spotify.app")).unwrap();
    std::fs::create_dir_all(apps.join("Adobe Photoshop 2026/Adobe Photoshop 2026.app")).unwrap();
    std::fs::create_dir_all(apps.join("Zoom Helper Tool.app")).unwrap();
    assert_eq!(find_apps(std::slice::from_ref(&apps), "spotify"), vec![apps.join("Spotify.app")]);
    assert_eq!(find_apps(std::slice::from_ref(&apps), "photoshop"), vec![apps.join("Adobe Photoshop 2026/Adobe Photoshop 2026.app")]);
    assert_eq!(find_apps(std::slice::from_ref(&apps), "zoom").len(), 2, "both match, so BYTE asks which");
    assert_eq!(find_apps(std::slice::from_ref(&apps), "zoom.us"), vec![apps.join("zoom.us.app")]);
    assert!(find_apps(std::slice::from_ref(&apps), "xy").is_empty(), "too short for a partial match");

    let plist = "<?xml version=\"1.0\"?><plist><dict><key>CFBundleName</key><string>zoom.us</string><key>CFBundleIdentifier</key>\n  <string>us.zoom.xos</string></dict></plist>";
    assert_eq!(bundle_id_xml(plist).as_deref(), Some("us.zoom.xos"));
    assert_eq!(bundle_id_xml("bplist00…"), None);

    let lib = d.path().join("Library");
    write(&lib.join("Preferences/us.zoom.xos.plist"), 100);
    write(&lib.join("Application Support/zoom.us/data.db"), 3_000);
    write(&lib.join("Caches/us.zoom.xos/c"), 2_000);
    std::fs::create_dir_all(lib.join("Containers/us.zoom.xos.Helper")).unwrap();
    std::fs::create_dir_all(lib.join("Group Containers/BJ4HAAB9B3.us.zoom.xos")).unwrap();
    write(&lib.join("Preferences/us.zoom.xosother.plist"), 100);
    write(&lib.join("Application Support/Zoomer/x"), 100);
    let left = leftovers(&lib, "us.zoom.xos", "zoom.us");
    let names: Vec<String> = left.iter().map(|(p, _)| p.strip_prefix(&lib).unwrap().display().to_string()).collect();
    assert_eq!(names, vec!["Application Support/zoom.us", "Caches/us.zoom.xos", "Preferences/us.zoom.xos.plist", "Containers/us.zoom.xos.Helper", "Group Containers/BJ4HAAB9B3.us.zoom.xos"]);
    let sizes: Vec<Option<u64>> = left.iter().map(|(_, s)| *s).collect();
    assert!(sizes[0].unwrap() >= 3_000 && sizes[3].is_none(), "protected folders aren't read");
}

#[test]
fn macos_apps_and_byte_itself_are_refused() {
    assert!(refuse_app(Path::new("/Applications/Safari.app"), "com.apple.Safari").is_some());
    assert!(refuse_app(Path::new("/System/Applications/Notes.app"), "").is_some());
    assert!(refuse_app(Path::new("/Applications/BYTE.app"), "com.loganstarner.byte").is_some());
    assert!(refuse_app(Path::new("/Applications/zoom.us.app"), "us.zoom.xos").is_none());
}

#[test]
fn login_items_are_read_and_matched() {
    let items = parse_login("Spotify\t/Applications/Spotify.app\nDropbox\t/Applications/Dropbox.app\n\n");
    assert_eq!(items.len(), 2);
    assert_eq!(match_login(&items, "spotify").unwrap().0, "Spotify");
    assert_eq!(match_login(&items, "the dropbox app").unwrap().0, "Dropbox");
    assert!(match_login(&items, "slack").is_none());
}

#[test]
fn scripts_take_the_users_words_only_as_arguments() {
    for s in SCRIPTS {
        assert!(s.starts_with("on run argv"), "{s}");
        assert!(!s.contains("do shell script"));
        assert!(!s.contains("empty trash") && !s.contains("empty the trash"), "never empties the Trash");
    }
}

/// A fake Mac: replies by program name or script.
#[derive(Default)]
struct Keyed {
    ran: StdMutex<Vec<Command>>,
    replies: StdMutex<HashMap<String, Result<String, RunError>>>,
}

impl Keyed {
    fn set(&self, key: &str, r: Result<String, RunError>) {
        self.replies.lock().unwrap().insert(key.into(), r);
    }
}

fn key(cmd: &Command) -> String {
    match cmd {
        Command::Exec { program, args } => format!("{program} {}", args.join(" ")).trim().to_string(),
        Command::Osa { script, .. } => {
            let names = [(TRASH, "TRASH"), (PUT_BACK, "PUT_BACK"), (FRONT_APPS, "FRONT_APPS"), (QUIT_APP, "QUIT_APP"), (QUIT_ID, "QUIT_ID"), (LOGIN_LIST, "LOGIN_LIST"), (LOGIN_REMOVE, "LOGIN_REMOVE"), (LOGIN_ADD, "LOGIN_ADD")];
            names.iter().find(|(s, _)| s == script).map(|(_, n)| n.to_string()).unwrap_or_else(|| "OTHER".into())
        }
    }
}

impl Runner for Keyed {
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        self.ran.lock().unwrap().push(cmd.clone());
        let k = key(cmd);
        let r = {
            let m = self.replies.lock().unwrap();
            m.get(&k).cloned().or_else(|| m.iter().find(|(key, _)| k.starts_with(key.as_str())).map(|(_, v)| v.clone())).unwrap_or(Err(RunError::Missing))
        };
        Box::pin(async move { r })
    }
}

fn busy_mac(k: &Keyed) {
    k.set("df", Ok("Filesystem 1024-blocks Used Available Capacity\n/dev/disk3s5 500000000 480000000 20000000 96% /\n".into()));
    k.set("memory_pressure", Ok("System-wide memory free percentage: 8%\n".into()));
    k.set("FRONT_APPS", Ok("Google Chrome\nFinder\n".into()));
    k.set("top", Ok("%CPU MEM COMMAND\n0 0 x\n%CPU  MEM    COMMAND\n97.3  2104M+ Google Chrome He\n12.0  512M   WindowServer\n".into()));
    k.set("pmset -g assertions", Ok("   pid 77(caffeinate): [0x3] 01:00:00 PreventUserIdleSystemSleep named: \"c\"\n".into()));
    k.set("pmset -g batt", Ok("Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t41%; discharging; 2:10 remaining present: true\n".into()));
    k.set("system_profiler", Ok("Cycle Count: 1012\nCondition: Service Recommended\nMaximum Capacity: 71%\n".into()));
    k.set("sysctl", Ok(format!("{{ sec = {}, usec = 0 }}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() - 20 * 86_400)));
    k.set("sw_vers", Ok("26.0\n".into()));
    k.set("tmutil", Err(RunError::Failed("No machine directory found".into())));
    k.set("fdesetup", Ok("FileVault is Off.\n".into()));
    k.set("socketfilterfw", Ok("Firewall is enabled. (State = 1)\n".into()));
}

#[tokio::test]
async fn the_coach_finds_what_slows_the_mac_and_only_offers_to_quit_apps() {
    let k = Keyed::default();
    busy_mac(&k);
    let h = health(&k, Path::new("/Users/ada"), true, SystemTime::now()).await;
    let level = |label: &str| h.checks.iter().find(|c| c.label == label).map(|c| c.level);
    assert_eq!(level("Storage"), Some(Level::Bad));
    assert_eq!(level("Memory"), Some(Level::Bad));
    assert_eq!(level("Processor"), Some(Level::Warn));
    assert_eq!(level("Battery health"), Some(Level::Bad));
    assert_eq!(level("Last restart"), Some(Level::Warn));
    assert_eq!(level("Keeping the Mac awake"), Some(Level::Info));
    assert_eq!(level("FileVault"), None, "the coach skips the check-up items");
    assert!(h.checks.windows(2).all(|w| w[0].level <= w[1].level), "worst first");
    let battery = h.checks.iter().find(|c| c.label == "Battery health").unwrap();
    assert!(battery.value.contains("71%") && battery.settings.as_deref().unwrap().contains("Battery"));
    assert_eq!(h.procs[0].app.as_deref(), Some("Google Chrome"));
    assert_eq!(h.procs[1].app, None, "WindowServer isn't an app");
    // The quit button only works for apps on this card.
    let ok = QUITTABLE.lock().unwrap().iter().any(|(id, apps)| *id == h.id && apps == &vec!["Google Chrome".to_string()]);
    assert!(ok);
}

#[tokio::test]
async fn the_checkup_covers_backups_encryption_and_firewall() {
    let k = Keyed::default();
    busy_mac(&k);
    let h = health(&k, Path::new("/Users/ada"), false, SystemTime::now()).await;
    let get = |label: &str| h.checks.iter().find(|c| c.label == label).cloned().unwrap();
    assert_eq!(get("Time Machine").level, Level::Warn);
    assert_eq!(get("FileVault").level, Level::Warn);
    assert_eq!(get("Firewall").level, Level::Ok);
    assert_eq!(get("macOS").value, "26.0");
    assert!(h.procs.is_empty() && !h.checks.iter().any(|c| c.label == "Processor"));
    // A Mac that can't answer at all still gives a card, just shorter.
    let empty = health(&Keyed::default(), Path::new("/Users/ada"), false, SystemTime::now()).await;
    assert!(empty.checks.iter().all(|c| c.label == "Time Machine"));
}

async fn flow(q: &str, fake: &Keyed, home: &Path, approve: Option<bool>) -> (Option<(SourceBook, String)>, Vec<ChatEvent>) {
    let dir = tempfile::tempdir().unwrap();
    let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
    let http = crate::chat::local_client();
    let ep = Endpoint { base_url: "http://127.0.0.1:9".into(), api_key: "k".into(), model: "m".into(), context: 8192, vision: false, cloud: None };
    let history = vec![crate::chat::ChatMessage::new("user", q)];
    let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, q);
    let turn = Turn {
        http: &http, cloud: None, net: &http, ep: &ep, system: "", history: &history, plan, mode: crate::settings::Mode::Auto, web: false, memory: false, log: &log, files: None,
        app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false,
        modules: crate::agent::Modules { mac: true, upkeep: true, ..Default::default() },
    };
    let seen: Arc<StdMutex<Vec<ChatEvent>>> = Arc::default();
    let s2 = seen.clone();
    let send = move |e: ChatEvent| {
        s2.lock().unwrap().push(e);
        Ok(())
    };
    if let Some(ok) = approve {
        let s3 = seen.clone();
        tokio::spawn(async move {
            loop {
                let id = s3.lock().unwrap().iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.id.clone()) } else { None });
                if let Some(id) = id {
                    crate::web_agent::answer(&id, ok);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
    }
    let out = run_with(&turn, q, fake, home, SystemTime::now(), &CancellationToken::new(), &send).await.unwrap();
    let ev = seen.lock().unwrap().clone();
    (out, ev)
}

#[tokio::test]
async fn a_storage_question_shows_the_card_and_remembers_what_may_go() {
    let d = tempfile::tempdir().unwrap();
    let old = write(&d.path().join("Downloads/Old.dmg"), 5_000);
    age(&old, 400);
    let k = Keyed::default();
    k.set("df", Ok("Filesystem 1024-blocks Used Available\n/dev/disk3s5 1000 900 100 90% /\n".into()));
    let (out, ev) = flow("what's taking up space on my mac", &k, d.path(), None).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Storage(s) = e { Some(s.clone()) } else { None }).unwrap();
    assert_eq!((card.total, card.free), (1000 * 1024, 100 * 1024));
    assert!(card.suggestions.iter().any(|s| s.id == "installers" && s.can_trash));
    assert!(out.unwrap().1.contains("Old installers in Downloads"));
    assert_eq!(take_allowed(&card.scan_id, "installers"), Some(vec![old]));
}

#[tokio::test]
#[cfg_attr(windows, ignore = "Upkeep is the Mac housekeeping feature: its fixtures, protection list and path handling are Mac-shaped. Not ported to Windows yet; it must be before the feature is offered there.")]
async fn uninstalling_asks_first_trashes_app_and_leftovers_and_can_be_undone() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let app = home.join("Applications/Spotify.app");
    write(&app.join("Contents/Info.plist"), 10);
    std::fs::write(app.join("Contents/Info.plist"), "<plist><dict><key>CFBundleIdentifier</key><string>com.spotify.client</string></dict></plist>").unwrap();
    let pref = write(&home.join("Library/Preferences/com.spotify.client.plist"), 100);
    let k = Keyed::default();
    k.set("QUIT_ID", Ok("ok".into()));
    k.set("TRASH", Ok(format!("{}/.Trash/Spotify.app\n{}/.Trash/com.spotify.client.plist\n", home.display(), home.display())));
    let (out, ev) = flow("uninstall spotify", &k, home, Some(true)).await;
    let ask = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).unwrap();
    assert_eq!(ask.title, "Uninstall Spotify");
    assert!(ask.fields.iter().any(|f| f.value.contains("com.spotify.client.plist")));
    let ran = k.ran.lock().unwrap().clone();
    assert!(matches!(&ran[0], Command::Osa { script, args } if *script == QUIT_ID && args == &vec!["com.spotify.client".to_string()]), "quits it first");
    let Command::Osa { args, .. } = &ran[1] else { panic!() };
    assert_eq!(args, &vec![app.display().to_string(), pref.display().to_string()]);
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(m) = e { Some(m.clone()) } else { None }).unwrap();
    assert!(done.ok && done.undo.is_some());
    assert!(out.unwrap().1.contains("moved Spotify and 1 of its files"));
}

#[tokio::test]
async fn saying_no_removes_nothing() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("Applications/Spotify.app")).unwrap();
    let k = Keyed::default();
    k.set("defaults", Ok("com.spotify.client".into()));
    let (out, _) = flow("uninstall spotify", &k, d.path(), Some(false)).await;
    assert!(!k.ran.lock().unwrap().iter().any(|c| matches!(c, Command::Osa { script, .. } if *script == TRASH || *script == QUIT_ID)));
    assert!(out.unwrap().1.contains("chose to keep Spotify"));
}

#[tokio::test]
async fn login_items_can_be_listed_and_removed_with_undo() {
    let d = tempfile::tempdir().unwrap();
    let k = Keyed::default();
    k.set("LOGIN_LIST", Ok("Spotify\t/Applications/Spotify.app\n".into()));
    let (out, ev) = flow("what opens at login", &k, d.path(), None).await;
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::Health(h) if h.title == "What opens at login")));
    assert!(out.unwrap().1.contains("Spotify"));

    k.set("LOGIN_REMOVE", Ok("/Applications/Spotify.app\tfalse".into()));
    let (out, ev) = flow("stop spotify from opening at login", &k, d.path(), Some(true)).await;
    let ran = k.ran.lock().unwrap().clone();
    assert!(ran.iter().any(|c| matches!(c, Command::Osa { script, args } if *script == LOGIN_REMOVE && args == &vec!["Spotify".to_string()])));
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(m) = e { Some(m.clone()) } else { None }).unwrap();
    assert!(done.undo.is_some());
    assert!(out.unwrap().1.contains("no longer opens at login"));

    let (out, ev) = flow("stop slack from opening at login", &k, d.path(), Some(true)).await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("isn't in the login items"));
}

/// On a Mac: the read-only tools run and their output parses.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore]
async fn e2e_health_on_a_real_mac() {
    let h = health(&MacRunner, &home(), true, SystemTime::now()).await;
    let labels: Vec<&str> = h.checks.iter().map(|c| c.label.as_str()).collect();
    assert!(labels.contains(&"Storage") && labels.contains(&"Memory") && labels.contains(&"Last restart"), "{labels:?}");
    let c = health(&MacRunner, &home(), false, SystemTime::now()).await;
    assert!(c.checks.iter().any(|x| x.label == "macOS"), "{:?}", c.checks);
}

/// The Windows Settings link goes to `cmd /C start`, so anything that could end the argument and start
/// another command must be refused: an ampersand, a pipe, a space, a quote, a percent escape, a path.
#[test]
fn windows_settings_links_cannot_smuggle_a_command() {
    for ok in ["ms-settings:privacy-microphone", "ms-settings:notifications", "ms-settings:privacy-webcam"] {
        assert!(windows_settings_link_ok(ok), "{ok}");
    }
    let long = format!("ms-settings:{}", "a".repeat(100));
    for bad in [
        "x-apple.systempreferences:com.apple.x",
        "ms-settings:privacy&calc",
        "ms-settings:privacy|calc",
        "ms-settings:a b",
        "ms-settings:\"quoted",
        "ms-settings:%0acalc",
        "ms-settings:..\\evil",
        "ms-settings:x;y",
        "http://example.com",
        "https://example.com/ms-settings:x",
        "",
        long.as_str(),
    ] {
        assert!(!windows_settings_link_ok(bad), "{bad:?} must be refused");
    }
}
