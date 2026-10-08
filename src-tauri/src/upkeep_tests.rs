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
    assert_eq!(applies(true, "free up space"), cfg!(any(target_os = "macos", windows)));
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
        Command::Win(_) => "WIN".into(),
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

async fn flow(q: &str, fake: &dyn Runner, home: &Path, approve: Option<bool>) -> (Option<(SourceBook, String)>, Vec<ChatEvent>) {
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

// ------------------------------------------------------------------ the same on a PC

/// A fake PC: answers the Recycle Bin operations and records every command.
#[derive(Default)]
struct PcFake {
    ran: StdMutex<Vec<Command>>,
    /// What the Recycle operation prints (one line per file); `ok` for each when unset.
    recycle: StdMutex<Option<String>>,
    /// What the Snapshot, Processes and Security operations print; unset means the PC can't answer.
    snapshot: StdMutex<Option<String>>,
    procs: StdMutex<Option<String>>,
    security: StdMutex<Option<String>>,
    /// What the startup list prints.
    startup: StdMutex<Option<String>>,
}

impl Runner for PcFake {
    fn pc(&self) -> bool {
        true
    }
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        self.ran.lock().unwrap().push(cmd.clone());
        let r = match cmd {
            Command::Win(WinOp::Recycle(p)) => Ok(self.recycle.lock().unwrap().clone().unwrap_or_else(|| vec!["ok"; p.len()].join("\n"))),
            Command::Win(WinOp::Restore(p)) => Ok(p.len().to_string()),
            Command::Win(WinOp::Snapshot) => self.snapshot.lock().unwrap().clone().ok_or(RunError::Missing),
            Command::Win(WinOp::Processes) => self.procs.lock().unwrap().clone().ok_or(RunError::Missing),
            Command::Win(WinOp::Security) => self.security.lock().unwrap().clone().ok_or(RunError::Missing),
            Command::Win(WinOp::StartupList) => self.startup.lock().unwrap().clone().ok_or(RunError::Missing),
            Command::Win(WinOp::StartupSet { on, .. }) => Ok(u8::from(*on).to_string()),
            _ => Err(RunError::Missing),
        };
        Box::pin(async move { r })
    }
}

#[test]
fn a_pc_reader_asks_the_same_questions_in_pc_words() {
    for q in ["what's taking up space on my PC?", "clean up my pc", "my disk is almost full", "find duplicate files", "clean up my computer"] {
        assert_eq!(ask(q), Some(Ask::Storage), "{q}");
    }
    for q in ["why is my PC so slow?", "my pc is running hot", "what's draining my battery"] {
        assert_eq!(ask(q), Some(Ask::Coach), "{q}");
    }
    for q in ["check my PC", "is my pc ok?", "run a health check"] {
        assert_eq!(ask(q), Some(Ask::Checkup), "{q}");
    }
    assert_eq!(ask("uninstall Zoom from my PC"), Some(Ask::Uninstall { app: "zoom".into() }));
    assert_eq!(ask("what opens at startup?"), Some(Ask::LoginItems));
    assert_eq!(ask("stop Spotify from opening at startup"), Some(Ask::LoginRemove { name: "spotify".into() }));
    // "pc" is a word, not a few letters inside another one.
    for q in ["what are the specs of this laptop", "explain how a pcb works", "the epcot park is slow in august"] {
        assert_eq!(ask(q), None, "{q}");
    }
}

#[test]
fn a_pcs_files_are_offered_and_protected_by_windows_rules() {
    let home = Path::new("C:\\Users\\ada");
    let ok = |p: &str| offerable_for(true, home, Path::new(p));
    assert!(ok("C:\\Users\\ada\\Videos\\film.mp4"));
    assert!(ok("c:/users/ADA/Downloads/report.pdf"), "case and slashes don't matter");
    assert!(!ok("C:\\Users\\ada\\AppData\\Local\\Mail\\x"), "AppData is the programs' own");
    assert!(!ok("C:\\Users\\ada\\.docker\\disk.raw"), "hidden folders");
    assert!(!ok("C:\\Users\\bob\\Downloads\\x.pdf"), "someone else's folder");
    assert!(!ok("D:\\Games\\x.bin"), "outside the user's folder");
    for p in [
        "C:\\Windows\\System32\\cmd.exe", "c:\\windows", "C:\\Program Files\\App\\app.exe", "C:\\Program Files (x86)\\Old", "C:\\ProgramData\\x",
        "C:\\$Recycle.Bin\\S-1\\x", "C:\\", "C:", "D:\\", "C:\\Users\\ada", "C:\\Users\\ada\\Documents", "c:/users/ada/downloads/", "C:\\Users\\ada\\AppData",
        "C:\\Users\\ada\\OneDrive", "C:\\Users\\ada\\Downloads\\..\\..\\..\\Windows", "relative\\path.txt", "Documents",
    ] {
        assert!(protected_pc(home, Path::new(p)), "{p}");
    }
    for p in ["C:\\Users\\ada\\Downloads\\old.msi", "C:\\Users\\ada\\Documents\\big.iso", "D:\\Games\\save.dat", "C:\\Users\\ada\\AppData\\Local\\pip\\Cache\\x"] {
        assert!(!protected_pc(home, Path::new(p)), "{p}");
    }
    // The Mac rules are untouched (Unix-style paths mean something only on a system that has them).
    if cfg!(unix) {
        assert!(protected_for(false, Path::new("/Users/ada"), Path::new("/System/Library")));
        assert!(!protected_for(false, Path::new("/Users/ada"), Path::new("/Users/ada/Downloads/a.dmg")));
    }
}

#[test]
fn a_pcs_installers_are_msi_and_iso_files_and_exes_named_setup() {
    for f in ["a.msi", "b.MSIX", "c.iso", "Zoom-Installer.exe", "vc_setup.exe", "setup.exe", "install_app.EXE"] {
        assert!(installer_like(true, Path::new(f)), "{f}");
    }
    for f in ["notepad++.exe", "game.exe", "a.zip", "readme.txt", "noext"] {
        assert!(!installer_like(true, Path::new(f)), "{f}");
    }
    assert!(installer_like(false, Path::new("a.dmg")) && !installer_like(false, Path::new("setup.exe")), "a Mac's are disk images");
}

#[test]
fn the_pc_scan_leaves_appdata_alone_and_finds_its_caches() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    write(&home.join("Videos/film.mp4"), 60_000);
    write(&home.join("Documents/report.pdf"), 20_000);
    let setup = write(&home.join("Downloads/Zoom-setup.exe"), 5_000);
    age(&setup, 90);
    write(&home.join("AppData/Roaming/Thing/huge.db"), 90_000);
    write(&home.join("AppData/Local/pip/Cache/wheel.whl"), 30_000);
    write(&home.join("AppData/Local/Temp/leftover.tmp"), 20_000);
    write(&home.join(".cargo/registry/cache/crate.crate"), 30_000);
    write(&home.join(".config/thing"), 4_000);
    let s = scan_for(true, home, SMALL.dup_min, Duration::from_secs(10), 100_000);
    let names: Vec<&str> = s.folders.iter().map(|f| f.0.as_str()).collect();
    assert!(names.contains(&"Videos") && names.contains(&"Documents") && names.contains(&"Downloads"), "{names:?}");
    assert!(!names.contains(&"AppData"), "AppData is not walked: {names:?}");
    // (Relative to the profile: on Windows the temporary folder this test runs in is itself inside AppData.)
    assert!(s.files.iter().all(|f| !f.path.strip_prefix(home).unwrap().to_string_lossy().contains("AppData")), "nothing inside AppData is offered");
    assert!(s.files.iter().any(|f| f.path.ends_with("film.mp4")));
    let labels: Vec<&str> = s.caches.iter().map(|c| c.0.as_str()).collect();
    assert!(labels.contains(&"Python package downloads") && labels.contains(&"Rust package downloads"), "{labels:?}");
    assert!(s.app_caches >= 20_000, "Temp is measured: {}", s.app_caches);
    // The same folder read the Mac way finds none of that.
    assert!(scan_for(false, home, SMALL.dup_min, Duration::from_secs(10), 100_000).caches.is_empty());
}

#[test]
fn the_pc_card_speaks_of_installers_temporary_files_and_the_recycle_bin() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let old = write(&home.join("Downloads/App-setup.exe"), 5_000);
    age(&old, 90);
    let msi = write(&home.join("Downloads/tool.msi"), 5_000);
    age(&msi, 90);
    write(&home.join("Downloads/notepad++.exe"), 5_000);
    age(&home.join("Downloads/notepad++.exe"), 90);
    let cache = home.join("AppData/Local/npm-cache");
    write(&cache.join("a.tgz"), 60_000);
    let scan = Scan { folders: vec![("Downloads".into(), home.join("Downloads"), 15_000)], files: vec![], caches: vec![("npm package cache".into(), cache.clone(), 60 << 20)], app_caches: 250 << 20, partial: false };
    let (card, allowed) = storage_card_for(true, home, &scan, &[], 1_000, 500, LIMITS, SystemTime::now());
    let get = |id: &str| card.suggestions.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no {id}: {:?}", card.suggestions.iter().map(|s| &s.id).collect::<Vec<_>>()));
    let inst = get("installers");
    assert_eq!(inst.count, 2, "the setup .exe and the .msi, not the program the person uses");
    assert!(inst.why.starts_with("Installers more than") && !inst.why.contains("Disk images"), "{}", inst.why);
    assert!(inst.items.iter().all(|i| i.starts_with("~\\")), "paths are written the PC's way: {:?}", inst.items);
    let temp = get("app-caches");
    assert_eq!((temp.title.as_str(), temp.can_trash, temp.items.clone()), ("Temporary files", false, vec!["%TEMP%".to_string()]));
    assert!(temp.why.contains("Storage Sense") && !temp.why.contains("macOS"));
    assert!(get("cache-0").why.starts_with("The tool rebuilds"));
    assert!(card.folders.iter().any(|f| f.name == "Temporary files" && f.path == "%TEMP%"));
    assert_eq!(allowed.get("installers").map(|p| p.len()), Some(2));
}

#[tokio::test]
async fn moving_to_the_recycle_bin_reports_what_moved_and_can_be_undone() {
    let d = tempfile::tempdir().unwrap();
    let a = write(&d.path().join("Downloads/a.msi"), 5_000);
    let b = write(&d.path().join("Downloads/b.msi"), 7_000);
    let fake = PcFake::default();
    *fake.recycle.lock().unwrap() = Some("ok\n!The file is in use by another program".into());
    let t = trash(&fake, d.path(), &[a.clone(), b.clone()]).await;
    // The bytes freed are what the file took on disk (a whole number of blocks), the same measure the scan used.
    assert_eq!((t.moved, t.bytes), (1, on_disk(&std::fs::metadata(&a).unwrap())));
    assert!(t.error.as_deref().unwrap().contains("b.msi") && t.error.as_deref().unwrap().contains("in use"), "{:?}", t.error);
    // Both went to the Recycle Bin operation in one call, and nothing else ran.
    assert_eq!(fake.ran.lock().unwrap().clone(), vec![Command::Win(WinOp::Recycle(vec![a.clone(), b.clone()]))]);
    // Undo restores exactly what moved.
    let step = macctl::undo_step(&t.undo.expect("an undo token"));
    assert_eq!(step, Some(Undo::Cmd(Command::Win(WinOp::Restore(vec![a])))));
}

#[tokio::test]
async fn nothing_protected_ever_reaches_the_recycle_bin() {
    let d = tempfile::tempdir().unwrap();
    let fake = PcFake::default();
    // The user's own top folder, a system path and a path that is not there.
    let t = trash(&fake, d.path(), &[d.path().to_path_buf(), PathBuf::from("C:\\Windows\\System32"), d.path().join("Downloads/gone.msi")]).await;
    assert_eq!(t.moved, 0);
    assert!(fake.ran.lock().unwrap().is_empty(), "no command was sent: {:?}", fake.ran.lock().unwrap());
    assert!(t.error.as_deref().unwrap().contains("aren't there"));
}

#[tokio::test]
async fn a_pc_storage_question_shows_the_card_and_remembers_what_may_go() {
    let d = tempfile::tempdir().unwrap();
    let old = write(&d.path().join("Downloads/Old-setup.exe"), 5_000);
    age(&old, 400);
    write(&d.path().join("Videos/film.mp4"), 60_000);
    let fake = PcFake::default();
    let (out, ev) = flow("what's taking up space on my PC", &fake, d.path(), None).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Storage(s) = e { Some(s.clone()) } else { None }).expect("a storage card");
    assert!(card.suggestions.iter().any(|s| s.id == "installers" && s.can_trash));
    assert!(card.folders.iter().any(|f| f.name == "Videos"));
    let notes = out.unwrap().1;
    assert!(notes.contains("this PC's storage") && notes.contains("Recycle Bin") && notes.contains("AppData"), "{notes}");
    assert!(!notes.contains("Mac") && !notes.contains("Trash") && !notes.contains("Finder"), "{notes}");
    assert_eq!(take_allowed(&card.scan_id, "installers"), Some(vec![old]));
    assert!(fake.ran.lock().unwrap().is_empty(), "looking changes nothing and asks the system for nothing");
}

#[tokio::test]
async fn the_rest_of_pc_upkeep_is_said_not_pretended() {
    let d = tempfile::tempdir().unwrap();
    for q in ["uninstall Zoom"] {
        let fake = PcFake::default();
        let (out, ev) = flow(q, &fake, d.path(), None).await;
        assert!(fake.ran.lock().unwrap().is_empty(), "{q}: nothing ran");
        assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Health(_) | ChatEvent::Storage(_))), "{q}: no card");
        assert!(out.unwrap().1.contains("can't yet"), "{q}");
    }
}

#[tokio::test]
async fn a_pc_slowness_question_shows_the_card_in_pc_words() {
    let d = tempfile::tempdir().unwrap();
    for (q, title, tool_what) in [("why is my PC so slow", "How your PC is doing", "slowing the PC"), ("check my PC", "PC check-up", "PC check-up")] {
        let f = PcFake::default();
        busy_pc(&f);
        let (out, ev) = flow(q, &f, d.path(), None).await;
        let card = ev.iter().find_map(|e| if let ChatEvent::Health(h) = e { Some(h.clone()) } else { None }).unwrap_or_else(|| panic!("{q}: no health card"));
        assert_eq!(card.title, title);
        let args = ev.iter().find_map(|e| if let ChatEvent::ToolCall { args, .. } = e { Some(args.to_string()) } else { None }).unwrap();
        assert!(args.contains(tool_what) && !args.contains("Mac"), "{args}");
        let notes = out.unwrap().1;
        assert!(notes.contains("this PC") && !notes.contains("Mac") && !notes.contains("Terminal"), "{notes}");
        assert!(ev.iter().any(|e| matches!(e, ChatEvent::ToolResult { ok: true, .. })));
    }
}

/// What the storage scan makes of the real profile of whoever runs it. Prints the card; checks only what must hold anywhere.
#[cfg(windows)]
#[test]
#[ignore = "walks the real user profile (up to 20 s); run with --ignored --nocapture"]
fn live_pc_the_storage_scan_of_this_profile() {
    let home = home();
    let started = Instant::now();
    let s = scan_for(true, &home, LIMITS.dup_min, SCAN_TIME, SCAN_ENTRIES);
    let dups = duplicates(&s.files, LIMITS.dup_min, Instant::now() + Duration::from_secs(15));
    let (total, free) = crate::system::disk_space_for(&home);
    let (card, allowed) = storage_card_for(true, &home, &s, &dups, total, free, LIMITS, SystemTime::now());
    println!("scan took {:.1}s; disk {} free of {}; partial={}", started.elapsed().as_secs_f32(), size_text(free), size_text(total), card.partial);
    for f in card.folders.iter().take(8) {
        println!("  folder {:<22} {:>10}  {}", f.name, size_text(f.bytes), f.path);
    }
    for sg in &card.suggestions {
        println!("  suggestion {:<28} {:>10} {} items, trash={}  {:?}", sg.title, size_text(sg.bytes), sg.count, sg.can_trash, sg.items.iter().take(2).collect::<Vec<_>>());
    }
    for b in card.big.iter().take(5) {
        println!("  big {} {} ({:?} days old)", b.path, size_text(b.bytes), b.days_old);
    }
    assert!(started.elapsed() < Duration::from_secs(60));
    assert!(total > 0 && free <= total);
    assert!(!card.folders.is_empty());
    // Nothing the card could offer to remove is protected, and nothing is inside AppData but the caches BYTE knows.
    for paths in allowed.values() {
        for p in paths {
            assert!(!protected_pc(&home, p), "{} is protected but offered", p.display());
        }
    }
    assert!(card.folders.iter().all(|f| f.name != "AppData"));
}

// ------------------------------------------------------- a PC's health

/// What the three readers printed on the real PC (an up-to-date desktop), with the disk line the check-up adds.
const PC_SNAPSHOT: &str = "mem=33409183744;20796620800\nboot=1791431648\nos=Windows 11 Home\ndisk=1000000000000;600000000000\n";
const PC_SECURITY: &str = "defender=True;True;Normal;1\nfirewall=Domain:True,Private:True,Public:True\nbitlocker=0\nupdate=2026-10-07\nreboot=0\n";

fn secs_ago(days: u64) -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() - days * 86_400
}

fn busy_pc(f: &PcFake) {
    f.snapshot.lock().unwrap().replace(format!(
        "mem=16000000000;1200000000\nboot={}\nos=Windows 11 Home\ndisk=500000000000;15000000000\npower=41;0;7800\n",
        secs_ago(20)
    ));
    f.procs.lock().unwrap().replace(
        "chrome.exe|97.3|2200000000|1\nsvchost.exe|12.0|90000000|0\nexplorer.exe|3.0|150000000|1\nbyte.exe|2.0|400000000|1\nSpotify.exe|1.5|300000000|1\n".into(),
    );
    f.security.lock().unwrap().replace("defender=False;False;Normal;40\nfirewall=Domain:True,Private:True,Public:False\nbitlocker=0\nupdate=2026-06-01\nreboot=1\nbattery=50000;35000\ncycles=612\n".into());
}

#[test]
fn a_pcs_snapshot_and_security_readings_are_parsed() {
    let s = parse_pc_snapshot(PC_SNAPSHOT);
    assert_eq!((s.mem_total, s.mem_free), (33_409_183_744, 20_796_620_800));
    assert_eq!(s.disk, Some((1_000_000_000_000, 600_000_000_000)));
    assert_eq!((s.boot, s.os.as_str(), s.power), (Some(1_791_431_648), "Windows 11 Home", None), "a desktop has no battery line");
    let laptop = parse_pc_snapshot("mem=1;1\npower=41;0;7800\n");
    assert_eq!(laptop.power, Some((41, false, Some(7800))));
    assert_eq!(parse_pc_snapshot("mem=1;1\npower=100;1;-1\n").power, Some((100, true, None)), "-1 means unknown");
    assert_eq!(parse_pc_snapshot("nonsense"), PcSnapshot::default());
    assert_eq!(parse_pc_snapshot("disk=0;0").disk, None, "a disk of no size is no disk");

    let sec = parse_pc_security(PC_SECURITY);
    assert_eq!(sec.defender, Some((true, true, "Normal".into(), 1)));
    assert_eq!(sec.firewall, vec![("Domain".into(), true), ("Private".into(), true), ("Public".into(), true)]);
    assert_eq!((sec.bitlocker, sec.reboot, sec.battery, sec.cycles), (Some(0), false, None, None));
    assert_eq!(sec.update, chrono::NaiveDate::from_ymd_opt(2026, 10, 7));
    assert_eq!(parse_pc_security("garbage"), PcSecurity::default());
}

#[test]
fn only_windowed_ordinary_programs_can_be_quit_from_the_card() {
    let (procs, quit) = pc_procs("chrome.exe|97.3|2200000000|1\nsvchost.exe|12.0|90000000|0\nexplorer.exe|3.0|150000000|1\nbyte.exe|2.0|400000000|1\nllama-server.exe|40|9000000000|0\nbad line\nx|y|z|1\n", false);
    let names: Vec<&str> = procs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Google Chrome", "BYTE's AI engine", "Svchost", "Explorer", "BYTE"], "busiest first; unreadable lines are skipped");
    assert_eq!(procs[0].app.as_deref(), Some("Google Chrome"));
    assert_eq!(procs[0].mem, size_text(2_200_000_000));
    for p in &procs[1..] {
        assert_eq!(p.app, None, "{} must not get a Quit button", p.name);
    }
    assert_eq!(quit, vec![("Google Chrome".to_string(), "chrome.exe".to_string())]);
    // When memory is what is short, the biggest come first; otherwise the busiest.
    let mix = "a.exe|90|1000|1\nb.exe|1|9000|1\nc.exe|50|5000|1\n";
    let order = |by_memory| pc_procs(mix, by_memory).0.into_iter().map(|p| p.name).collect::<Vec<_>>();
    assert_eq!(order(false), ["A", "C", "B"]);
    assert_eq!(order(true), ["B", "C", "A"]);
    for never in ["explorer.exe", "EXPLORER", "svchost.exe", "winlogon.exe", "byte.exe", "powershell.exe", "msedgewebview2.exe", "System"] {
        assert!(!quittable_pc(never), "{never}");
    }
    assert!(quittable_pc("Spotify.exe") && quittable_pc("notepad"));
    assert_eq!(pretty_proc("MSEdge.exe"), "Microsoft Edge");
    assert_eq!(pretty_proc("7zFM.exe"), "7zFM");
    assert_eq!(pretty_proc("İstanbul.EXE"), "İstanbul", "a letter that lowercases to more bytes must not break the cut");
    assert_eq!(pretty_proc(".exe"), "", "nothing left is nothing to show");
    assert!(quittable_pc("İstanbul.exe") && quittable_pc("é"));
}

#[tokio::test]
async fn the_pc_coach_finds_what_slows_it_and_only_offers_to_quit_programs() {
    let f = PcFake::default();
    busy_pc(&f);
    let h = health(&f, Path::new("C:\\Users\\ada"), true, SystemTime::now()).await;
    let get = |label: &str| h.checks.iter().find(|c| c.label == label).cloned();
    assert_eq!(get("Storage").unwrap().level, Level::Bad);
    assert_eq!(get("Memory").unwrap().level, Level::Bad);
    assert_eq!(get("Processor").unwrap().level, Level::Warn);
    assert!(get("Processor").unwrap().value.contains("Google Chrome (97%)"));
    assert_eq!(get("Last restart").unwrap().level, Level::Warn);
    assert_eq!(get("Battery health").unwrap().level, Level::Warn, "70% of new");
    assert!(get("Battery health").unwrap().value.contains("70%") && get("Battery health").unwrap().value.contains("612"));
    assert!(get("Battery").unwrap().value.contains("on battery") && get("Battery").unwrap().value.contains("2h 10m"));
    assert!(get("Virus protection").is_none() && get("Firewall").is_none(), "the coach skips the check-up items");
    assert!(h.checks.windows(2).all(|w| w[0].level <= w[1].level), "worst first");
    // The settings links are Windows ones.
    let storage = get("Storage").unwrap();
    assert!(storage.settings.as_deref().unwrap().starts_with("ms-settings:"), "{:?}", storage.settings);
    assert_eq!(h.procs[0].app.as_deref(), Some("Google Chrome"));
    assert_eq!(h.procs[1].app, None, "a service isn't a program");
    let known = PC_QUIT.lock().unwrap().iter().any(|(id, m)| *id == h.id && m == &vec![("Google Chrome".to_string(), "chrome.exe".to_string()), ("Spotify".to_string(), "Spotify.exe".to_string())]);
    assert!(known, "the card remembers which program each Quit button means");
    let listed = QUITTABLE.lock().unwrap().iter().any(|(id, apps)| *id == h.id && apps == &vec!["Google Chrome".to_string(), "Spotify".to_string()]);
    assert!(listed);
    assert!(f.ran.lock().unwrap().iter().any(|c| matches!(c, Command::Win(WinOp::Security))), "a battery's wear needs the slow reader");
}

#[tokio::test]
async fn the_pc_checkup_covers_security_updates_and_encryption() {
    let f = PcFake::default();
    busy_pc(&f);
    let h = health(&f, Path::new("C:\\Users\\ada"), false, SystemTime::now()).await;
    let get = |label: &str| h.checks.iter().find(|c| c.label == label).cloned().unwrap();
    assert_eq!(get("Virus protection").level, Level::Bad);
    assert_eq!(get("Windows Update").level, Level::Warn, "a restart is waiting");
    assert_eq!(get("Firewall").level, Level::Warn, "public networks are off");
    assert_eq!(get("Disk encryption").level, Level::Info);
    assert_eq!(get("Disk encryption").settings.as_deref(), Some(crate::pcctl::settings_uri("encryption").as_str()));
    assert_eq!(get("Windows").value, "Windows 11 Home");
    assert!(h.procs.is_empty() && !h.checks.iter().any(|c| c.label == "Processor"));
    assert!(h.checks.windows(2).all(|w| w[0].level <= w[1].level));

    // A healthy desktop: nothing to worry about.
    let ok = PcFake::default();
    ok.snapshot.lock().unwrap().replace(PC_SNAPSHOT.replace("boot=1791431648", &format!("boot={}", secs_ago(2))));
    ok.security.lock().unwrap().replace(PC_SECURITY.replace("2026-10-07", &chrono::Local::now().format("%Y-%m-%d").to_string()));
    let h = health(&ok, Path::new("C:\\Users\\ada"), false, SystemTime::now()).await;
    for label in ["Storage", "Memory", "Last restart", "Virus protection", "Windows Update", "Firewall"] {
        let c = h.checks.iter().find(|c| c.label == label).unwrap_or_else(|| panic!("{label}"));
        assert_eq!(c.level, Level::Ok, "{label}: {c:?}");
    }
    assert!(h.checks.iter().all(|c| c.level != Level::Bad && c.level != Level::Warn));
    assert!(!h.checks.iter().any(|c| c.label.starts_with("Battery")), "a desktop has no battery");

    // Another antivirus in charge is not an alarm.
    let other = PcFake::default();
    other.security.lock().unwrap().replace("defender=True;False;Passive mode;3\n".into());
    let h = health(&other, Path::new("C:\\Users\\ada"), false, SystemTime::now()).await;
    assert_eq!(h.checks.iter().find(|c| c.label == "Virus protection").unwrap().level, Level::Info);

    // A PC that can't answer at all still gives a (shorter) card.
    let empty = health(&PcFake::default(), Path::new("C:\\Users\\ada"), false, SystemTime::now()).await;
    assert!(empty.checks.iter().all(|c| matches!(c.label.as_str(), "Backups" | "Virus protection")), "{:?}", empty.checks);
}

/// The coach and the check-up on the real PC, with the real readers. Prints both cards; checks only what must hold anywhere.
#[cfg(windows)]
#[tokio::test]
#[ignore = "runs the real Windows readers (about 10 s); run with --ignored --nocapture in the desktop session"]
async fn live_pc_the_checkup_of_this_pc() {
    let runner = crate::pcctl::WinRunner;
    for coach in [true, false] {
        let started = Instant::now();
        let h = health(&runner, &home(), coach, SystemTime::now()).await;
        println!("--- {} ({:.1} s)", h.title, started.elapsed().as_secs_f32());
        for c in &h.checks {
            println!("  [{:?}] {}: {}{}{}", c.level, c.label, c.value, if c.tip.is_empty() { String::new() } else { format!("  -- {}", c.tip) }, c.settings.as_deref().map(|s| format!("  <{s}>")).unwrap_or_default());
        }
        for p in &h.procs {
            println!("  proc {} {:.0}% {} quit={:?}", p.name, p.cpu, p.mem, p.app);
        }
        assert!(h.checks.iter().any(|c| c.label == "Storage"), "the disk is always readable");
        assert!(h.checks.iter().any(|c| c.label == "Memory"));
        assert!(h.checks.iter().any(|c| c.label == "Last restart"));
        assert!(h.checks.windows(2).all(|w| w[0].level <= w[1].level));
        assert_eq!(h.procs.is_empty(), !coach, "only the coach lists busy programs");
        assert!(h.procs.iter().all(|p| p.app.as_deref().map_or(true, |a| crate::upkeep::quittable_pc(&PC_QUIT.lock().unwrap().iter().find(|(id, _)| *id == h.id).and_then(|(_, m)| m.iter().find(|(d, _)| d == a).map(|(_, e)| e.clone())).unwrap()))));
        if !coach {
            assert!(h.checks.iter().any(|c| c.label == "Virus protection"));
            assert!(h.checks.iter().any(|c| c.label == "Windows Update"));
        }
    }
}

// ------------------------------------------------------- what starts with Windows

/// What the startup list printed on the real PC (the user name changed), one entry per line, tab-separated.
const PC_STARTUP: &str = "user\trun\tOneDrive\t\"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background\t0
user\trun\tSteam\t\"C:\\Program Files (x86)\\Steam\\steam.exe\" -silent\t0
user\trun\tDiscord\t\"C:\\Users\\ada\\AppData\\Local\\Discord\\Update.exe\" --processStart Discord.exe\t0
user\trun\tEADM\t\"C:\\Program Files\\Electronic Arts\\EA Desktop\\EA Desktop\\EALauncher.exe\" -silentOs\t0
user\trun\telectron.app.CurseForge\tC:\\Users\\ada\\AppData\\Local\\Programs\\CurseForge Windows\\CurseForge.exe --minimized\t0
user\trun\tUnified Remote V3\t\"C:\\Program Files (x86)\\Unified Remote 3\\RemoteServerWin.exe\"\t1
user\trun\tParsec.App.0\tC:\\Program Files\\Parsec\\parsecd.exe app_silent=1\t1
user\trun\tMicrosoftEdgeAutoLaunch_A1306234171FE4BFED863ECABC261099\t\"C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe\" --no-startup-window --win-session-start\t0
machine\trun\tSecurityHealth\tC:\\WINDOWS\\system32\\SecurityHealthSystray.exe\t1
machine\trun\tRtkAudUService\t\"C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\realtekservice.inf_amd64_8f3e2cb35a0fd6a8\\RtkAudUService64.exe\" -background\t1
machine\trun\tStartAUEP\t\"C:\\Program Files\\AMD\\Performance Profile Client\\AUEPMaster.exe\"\t1
machine\trun\tCorsair iCUE5 Software\t\"C:\\Program Files\\Corsair\\Corsair iCUE5 Software\\iCUE Launcher.exe\" --autorun\t1
user\tfolder\tOllama.lnk\tC:\\Users\\ada\\AppData\\Local\\Programs\\Ollama\\ollama app.exe\t0
machine\tfolder\tTailscale.lnk\tC:\\Program Files\\Tailscale\\tailscale-ipn.exe\t1
";

#[test]
fn the_startup_list_is_read_named_and_matched() {
    let items = parse_startup(PC_STARTUP);
    assert_eq!(items.len(), 14);
    assert_eq!(items.iter().filter(|i| i.on).count(), 7);
    assert_eq!((items[0].scope.as_str(), items[0].kind.as_str(), items[0].name.as_str(), items[0].on), ("user", "run", "OneDrive", false));
    assert_eq!(items[0].command, "\"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background");
    assert_eq!(items[12].kind, "folder");
    // Lines that are not entries are skipped.
    assert!(parse_startup("garbage\nuser\tnope\tx\ty\t1\nuser\trun\t\tcmd\t1\n").is_empty());

    let titles: Vec<String> = items.iter().map(startup_title).collect();
    assert_eq!(
        titles,
        ["OneDrive", "Steam", "Discord", "EADM", "CurseForge", "Unified Remote V3", "Parsec", "Microsoft Edge", "SecurityHealth", "RtkAudUService", "StartAUEP", "Corsair iCUE5 Software", "Ollama", "Tailscale"]
    );
    for (cmd, exe) in [
        ("\"C:\\Program Files\\X\\x.exe\" -silent", "x.exe"),
        ("C:\\Program Files\\Parsec\\parsecd.exe app_silent=1", "parsecd.exe"),
        ("C:\\Users\\ada\\AppData\\Local\\Programs\\Ollama\\ollama app.exe", "ollama app.exe"),
        ("\"C:\\a b\\Tool.EXE\"", "Tool.EXE"),
        ("cmd /c start notepad", "cmd"),
        ("", ""),
    ] {
        assert_eq!(exe_of_command(cmd), exe, "{cmd}");
    }
    let find = |w: &str| match_startup(&items, w).map(|i| i.name.clone());
    assert_eq!(find("discord").as_deref(), Some("Discord"));
    assert_eq!(find("Discord").as_deref(), Some("Discord"));
    assert_eq!(find("edge").as_deref(), Some("MicrosoftEdgeAutoLaunch_A1306234171FE4BFED863ECABC261099"));
    assert_eq!(find("parsec").as_deref(), Some("Parsec.App.0"));
    assert_eq!(find("curseforge").as_deref(), Some("electron.app.CurseForge"));
    assert_eq!(find("ollama").as_deref(), Some("Ollama.lnk"));
    assert_eq!(find("icue").as_deref(), Some("Corsair iCUE5 Software"));
    assert_eq!(find("onedrive").as_deref(), Some("OneDrive"));
    assert_eq!(find("photoshop"), None);
    assert_eq!(find("x"), None, "one letter matches everything");

    // Windows and the drivers' entries, and BYTE's own, are never offered.
    let why = |n: &str| protected_startup(items.iter().find(|i| i.name == n).unwrap());
    assert!(why("SecurityHealth").is_some() && why("RtkAudUService").is_some());
    for n in ["Discord", "Corsair iCUE5 Software", "Parsec.App.0"] {
        assert!(why(n).is_none(), "{n}");
    }
    let own = StartupItem { scope: "user".into(), kind: "run".into(), name: "BYTE".into(), command: "\"C:\\Users\\ada\\AppData\\Local\\BYTE\\byte.exe\" --hidden".into(), on: true };
    assert!(protected_startup(&own).unwrap().contains("BYTE's own"));
}

#[test]
fn pc_phrases_for_startup_apps_are_understood() {
    for q in ["what opens at startup?", "what starts with windows", "show my startup apps", "which programs run when windows starts", "list the startup programs"] {
        assert_eq!(ask(q), Some(Ask::LoginItems), "{q}");
    }
    for (q, name) in [
        ("stop Spotify from opening at startup", "spotify"),
        ("stop discord from starting with windows", "discord"),
        ("don't launch steam when windows starts", "steam"),
        ("disable parsec at startup", "parsec"),
    ] {
        assert_eq!(ask(q), Some(Ask::LoginRemove { name: name.into() }), "{q}");
    }
}

#[tokio::test]
async fn the_pc_startup_card_lists_what_opens_and_what_is_off() {
    let d = tempfile::tempdir().unwrap();
    let f = PcFake::default();
    f.startup.lock().unwrap().replace(PC_STARTUP.into());
    let (out, ev) = flow("what opens at startup?", &f, d.path(), None).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Health(h) = e { Some(h.clone()) } else { None }).expect("a card");
    assert_eq!(card.title, "What opens when Windows starts");
    let labels: Vec<&str> = card.checks.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels.len(), 15, "14 entries and the settings row");
    let first_off = card.checks.iter().position(|c| c.value.starts_with("Off")).unwrap();
    assert!(card.checks[..first_off].iter().all(|c| c.value.starts_with("Opens at startup")), "open ones first");
    let get = |l: &str| card.checks.iter().find(|c| c.label == l).unwrap();
    assert!(get("Parsec").value.contains("parsecd.exe") && get("Corsair iCUE5 Software").value.contains("for everyone"));
    assert!(get("Discord").value.starts_with("Off"));
    assert!(get("SecurityHealth").tip.contains("leaves this one alone"));
    assert!(get("Change these").settings.as_deref().unwrap().starts_with("ms-settings:"));
    let notes = out.unwrap().1;
    assert!(notes.contains("Open when Windows starts: ") && notes.contains("Parsec") && notes.contains("Turned off already: ") && notes.contains("Discord"), "{notes}");
    assert!(!notes.contains("login") && !notes.contains("Mac"), "{notes}");
    let ran = f.ran.lock().unwrap().clone();
    assert_eq!(ran, vec![Command::Win(WinOp::StartupList)], "looking changes nothing");
}

#[tokio::test]
async fn turning_off_a_startup_app_asks_first_and_can_be_undone() {
    let d = tempfile::tempdir().unwrap();
    let f = PcFake::default();
    f.startup.lock().unwrap().replace(PC_STARTUP.into());
    // Saying no changes nothing.
    let (out, ev) = flow("stop parsec from opening at startup", &f, d.path(), Some(false)).await;
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(!f.ran.lock().unwrap().iter().any(|c| matches!(c, Command::Win(WinOp::StartupSet { .. }))));
    assert!(out.unwrap().1.contains("Nothing changed"));
    // Yes turns that one entry off, by the name Windows keeps it under, and Undo turns it back on.
    let (out, ev) = flow("stop parsec from opening at startup", &f, d.path(), Some(true)).await;
    let set = Command::Win(WinOp::StartupSet { kind: "run".into(), name: "Parsec.App.0".into(), on: false });
    assert!(f.ran.lock().unwrap().contains(&set));
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(m) = e { Some(m.clone()) } else { None }).expect("a done card");
    assert!(done.ok && done.title.contains("Parsec") && done.title.contains("Windows starts"), "{done:?}");
    let notes = out.unwrap().1;
    assert!(notes.contains("no longer opens when Windows starts") && !notes.contains("login"), "{notes}");
    match macctl::undo_step(&done.undo.expect("an undo token")) {
        Some(macctl::Undo::Cmd(Command::Win(WinOp::StartupSet { kind, name, on }))) => assert_eq!((kind.as_str(), name.as_str(), on), ("run", "Parsec.App.0", true)),
        other => panic!("undo should turn it back on, got {other:?}"),
    }
    // A folder shortcut is switched the same way, under its own kind.
    let (_, _) = flow("stop corsair icue from opening at startup", &f, d.path(), Some(true)).await;
    assert!(f.ran.lock().unwrap().contains(&Command::Win(WinOp::StartupSet { kind: "run".into(), name: "Corsair iCUE5 Software".into(), on: false })));
}

#[tokio::test]
async fn startup_entries_that_are_off_or_belong_to_windows_or_are_unknown_are_not_changed() {
    let d = tempfile::tempdir().unwrap();
    let f = PcFake::default();
    f.startup.lock().unwrap().replace(PC_STARTUP.into());
    for (q, says) in [
        ("stop discord from opening at startup", "already turned off"),
        ("stop securityhealth from opening at startup", "belongs to Windows itself"),
        ("stop rtkaud from opening at startup", "belongs to Windows itself"),
        ("stop photoshop from opening at startup", "isn't in the startup list"),
    ] {
        let (out, ev) = flow(q, &f, d.path(), Some(true)).await;
        assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))), "{q}: nothing to approve");
        assert!(out.unwrap().1.contains(says), "{q}");
    }
    assert!(!f.ran.lock().unwrap().iter().any(|c| matches!(c, Command::Win(WinOp::StartupSet { .. }))));
    // A PC that cannot read the list says so; it does not pretend.
    let blind = PcFake::default();
    let (out, ev) = flow("what opens at startup?", &blind, d.path(), None).await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Health(_))));
    assert!(out.unwrap().1.contains("couldn't read the startup list"));
}

/// "What opens at startup?" on the real PC, through the whole flow. Prints the card; changes nothing.
#[cfg(windows)]
#[tokio::test]
#[ignore = "reads the real startup entries; run with --ignored --nocapture"]
async fn live_pc_the_startup_card_of_this_pc() {
    let (out, ev) = flow("what opens at startup?", &crate::pcctl::WinRunner, &home(), None).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Health(h) = e { Some(h.clone()) } else { None }).expect("a card");
    println!("--- {}", card.title);
    for c in &card.checks {
        println!("  {}: {}{}", c.label, c.value, if c.tip.is_empty() { String::new() } else { format!("  -- {}", c.tip) });
    }
    println!("--- notes:\n{}", out.unwrap().1);
    assert!(card.checks.len() >= 2 && card.checks.last().unwrap().label == "Change these");
}
