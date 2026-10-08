use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime};

use super::*;
use crate::engine::Endpoint;
use crate::macctl::RunError;
use crate::pcctl::WinOp;

#[test]
fn file_requests_are_recognized_and_others_are_not() {
    assert_eq!(ask("Find my tax return pdf"), Some(Ask::Find { query: "tax return".into() }));
    assert_eq!(ask("where's my resume?"), Some(Ask::Find { query: "resume".into() }));
    assert_eq!(ask("find the lease document on my mac"), Some(Ask::Find { query: "lease".into() }));
    assert_eq!(ask("organize my Downloads"), Some(Ask::Organize { folder: "Downloads".into(), by: By::Kind }));
    assert_eq!(ask("tidy my desktop by month"), Some(Ask::Organize { folder: "Desktop".into(), by: By::Month }));
    assert_eq!(ask("convert the selected photos to jpg"), Some(Ask::Convert { to: Some("jpg".into()), max: None }));
    assert_eq!(ask("make the selected photos smaller"), Some(Ask::Convert { to: None, max: Some(1600) }));
    assert_eq!(ask("resize the selected images to 1200"), Some(Ask::Convert { to: None, max: Some(1200) }));
    assert_eq!(ask("summarize the files I selected in Finder"), Some(Ask::AboutSelection));
    for q in ["find a recipe for banana bread", "find coffee near me", "how do I organize my downloads?", "find cheap flights to Lisbon", "organize my week", "where is the Eiffel Tower"] {
        assert_eq!(ask(q), None, "{q}");
    }
}

#[test]
fn files_are_grouped_by_kind() {
    assert_eq!(kind_folder(Path::new("a/IMG_1234.HEIC")), "Images");
    assert_eq!(kind_folder(Path::new("Lease 2026.pdf")), "Documents");
    assert_eq!(kind_folder(Path::new("Zoom.pkg")), "Installers");
    assert_eq!(kind_folder(Path::new("notes.xyz")), "Other");
    assert_eq!(kind_folder(Path::new("README")), "Other");
}

fn touch(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, name).unwrap();
    p
}

#[test]
fn tidying_moves_loose_files_and_undo_puts_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for n in ["a.jpg", "b.pdf", "c.dmg", "d.zip", ".hidden", "e.crdownload", "f.pdf"] {
        touch(d, n);
    }
    std::fs::create_dir(d.join("Old folder")).unwrap();
    std::fs::create_dir(d.join("Documents")).unwrap();
    touch(&d.join("Documents"), "f.pdf"); // already there: the moved one gets a new name
    let later = SystemTime::now() + Duration::from_secs(3600);
    let plan = tidy_plan(d, By::Kind, later).unwrap();
    // Windows spells the separator with a backslash; the test is about WHERE files go.
    let names: Vec<String> = plan.iter().map(|(_, to)| to.strip_prefix(d).unwrap().display().to_string().replace('\\', "/")).collect();
    assert_eq!(names, ["Images/a.jpg", "Documents/b.pdf", "Installers/c.dmg", "Archives/d.zip", "Documents/f 2.pdf"]);
    assert_eq!(plan_summary(&plan), "Documents: 2 · Archives: 1 · Images: 1 · Installers: 1");
    // Just-downloaded files are left alone.
    assert!(tidy_plan(d, By::Kind, SystemTime::now()).unwrap().is_empty());
    let (done, err) = apply_moves(&plan);
    assert!(err.is_none() && done.len() == 5);
    assert!(d.join("Images/a.jpg").exists() && !d.join("a.jpg").exists());
    assert!(d.join(".hidden").exists() && d.join("e.crdownload").exists() && d.join("Old folder").is_dir());
    assert_eq!(std::fs::read_to_string(d.join("Documents/f.pdf")).unwrap(), "f.pdf", "nothing overwritten");
    assert_eq!(crate::macctl::move_back(&done), 5);
    assert!(d.join("a.jpg").exists() && d.join("f.pdf").exists() && !d.join("Documents/f 2.pdf").exists());
}

#[test]
fn converting_keeps_originals_and_passes_paths_as_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let a = touch(d, "IMG 1.heic");
    let b = touch(d, "notes.txt");
    touch(d, "IMG 1.jpg");
    let plan = convert_plan(&[a.clone(), b], Some("jpg"), None);
    assert_eq!(plan.len(), 1, "only photos");
    assert_eq!(plan[0].1, d.join("IMG 1 2.jpg"), "never over an existing file");
    assert_eq!(plan[0].2, Some("jpeg"));
    let small = convert_plan(std::slice::from_ref(&a), None, Some(1600));
    assert_eq!(small[0].1, d.join("IMG 1 (small).heic"));
    let evil = d.join("x\"; rm -rf ~; \".jpg");
    let Command::Exec { program, args } = sips_command(&evil, &d.join("o.png"), Some("png"), Some(800)) else { panic!() };
    assert_eq!(program, "sips");
    assert_eq!(args[..4], ["-s", "format", "png", "-Z"]);
    assert!(args.contains(&evil.display().to_string()), "the path is one argument, no shell");
}

/// An hour from now, so that files made a moment ago are not "still downloading".
fn now_for_tests() -> SystemTime {
    SystemTime::now() + Duration::from_secs(3600)
}

#[derive(Default)]
struct Fake {
    ran: StdMutex<Vec<Command>>,
    replies: StdMutex<Vec<Result<String, RunError>>>,
}

impl Runner for Fake {
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        self.ran.lock().unwrap().push(cmd.clone());
        let r = {
            let mut r = self.replies.lock().unwrap();
            if r.is_empty() {
                Ok(String::new())
            } else {
                r.remove(0)
            }
        };
        Box::pin(async move { r })
    }
}

async fn flow(q: &str, fake: &Fake, approve: Option<bool>) -> (Option<(SourceBook, String)>, Vec<ChatEvent>) {
    flow_in(q, fake, &home(), approve).await
}

async fn flow_in(q: &str, fake: &dyn Runner, home: &Path, approve: Option<bool>) -> (Option<(SourceBook, String)>, Vec<ChatEvent>) {
    let dir = tempfile::tempdir().unwrap();
    let log = crate::tools::ActionLog::new(dir.path().join("a.jsonl"));
    let http = crate::chat::local_client();
    let ep = Endpoint { base_url: "http://127.0.0.1:9".into(), api_key: "k".into(), model: "m".into(), context: 8192, vision: false, cloud: None };
    let history = vec![crate::chat::ChatMessage::new("user", q)];
    let plan = crate::router::plan_turn(crate::settings::Mode::Auto, crate::settings::ThinkingPref::Off, q);
    let turn = Turn {
        http: &http, cloud: None, net: &http, ep: &ep, system: "", history: &history, plan, mode: crate::settings::Mode::Auto, web: false, memory: false, log: &log, files: None,
        app: None, task: None, home: None, depth: 0, web_always: false, kitchen: false, metric: false, agent: false,
        modules: crate::agent::Modules { mac: true, ..Default::default() },
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
    let out = run_with(&turn, q, fake, home, now_for_tests(), &CancellationToken::new(), &send).await.unwrap();
    let ev = seen.lock().unwrap().clone();
    (out, ev)
}

#[tokio::test]
async fn converting_the_finder_selection_asks_first_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let a = touch(dir.path(), "beach.heic");
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(format!("{}\n", a.display())));
    let (out, ev) = flow("convert the selected photos to jpg", &fake, Some(true)).await;
    let card = ev.iter().find_map(|e| if let ChatEvent::Approval(x) = e { Some(x.clone()) } else { None }).unwrap();
    assert_eq!(card.title, "Save 1 photos as JPG");
    let ran = fake.ran.lock().unwrap().clone();
    assert_eq!(ran[1], sips_command(&a, &dir.path().join("beach.jpg"), Some("jpeg"), None));
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(d) = e { Some(d.clone()) } else { None }).unwrap();
    assert!(done.ok && done.undo.is_some());
    assert!(out.unwrap().1.contains("saved 1 new photo files"));
}

#[tokio::test]
async fn nothing_selected_changes_nothing() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok(String::new()));
    let (out, ev) = flow("make the selected photos smaller", &fake, None).await;
    assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("No photos are selected"));
}

#[tokio::test]
async fn finding_uses_spotlight_with_the_words_as_one_argument() {
    let fake = Fake::default();
    fake.replies.lock().unwrap().push(Ok("/Users/ada/Documents/Taxes/2025 return.pdf\n/Users/ada/Library/Caches/x.pdf\n".into()));
    let (out, _) = flow("find my tax return pdf", &fake, None).await;
    let ran = fake.ran.lock().unwrap().clone();
    let Command::Exec { program, args } = &ran[0] else { panic!() };
    assert_eq!((program.to_string(), args.last().unwrap().as_str()), ("mdfind".to_string(), "tax return"));
    let notes = out.unwrap().1;
    assert!(notes.contains("2025 return.pdf") && !notes.contains("Caches"), "{notes}");
}

/// On a Mac: the Finder script compiles.
#[cfg(target_os = "macos")]
#[test]
#[ignore]
fn e2e_finder_script_compiles() {
    let dir = tempfile::tempdir().unwrap();
    for (i, s) in SCRIPTS.iter().enumerate() {
        let out = std::process::Command::new("/usr/bin/osacompile").arg("-o").arg(dir.path().join(format!("{i}.scpt"))).arg("-e").arg(s).output().unwrap();
        assert!(out.status.success(), "script {i}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

// ------------------------------------------------------------------ the same on a PC

/// A fake PC: answers the search with what the test set, and records every command.
#[derive(Default)]
struct PcFiles {
    ran: StdMutex<Vec<Command>>,
    found: StdMutex<Option<Result<String, RunError>>>,
    /// What File Explorer says is selected (one path per line); unset means it cannot say.
    selected: StdMutex<Option<Result<String, RunError>>>,
    /// Per photo, in order: what the conversion says (`Ok("size")` when unset).
    converted: StdMutex<Vec<Result<String, RunError>>>,
}

impl Runner for PcFiles {
    fn pc(&self) -> bool {
        true
    }
    fn run<'a>(&'a self, cmd: &'a Command) -> futures_util::future::BoxFuture<'a, Result<String, RunError>> {
        self.ran.lock().unwrap().push(cmd.clone());
        let r = match cmd {
            Command::Win(WinOp::FindFiles { .. }) => self.found.lock().unwrap().clone().unwrap_or(Err(RunError::Missing)),
            Command::Win(WinOp::ExplorerSelection) => self.selected.lock().unwrap().clone().unwrap_or(Err(RunError::Missing)),
            Command::Win(WinOp::ConvertPhoto { .. }) => {
                let mut c = self.converted.lock().unwrap();
                if c.is_empty() { Ok("1200x800".into()) } else { c.remove(0) }
            }
            _ => Err(RunError::Missing),
        };
        Box::pin(async move { r })
    }
}

#[test]
fn a_pc_reader_asks_for_files_in_pc_words() {
    assert_eq!(ask("find my tax return pdf on my pc"), Some(Ask::Find { query: "tax return".into() }));
    assert_eq!(ask("where's my resume on this computer?"), Some(Ask::Find { query: "resume".into() }));
    assert_eq!(ask("search my pc for the lease document"), Some(Ask::Find { query: "lease".into() }));
    assert_eq!(ask("find the lease file in file explorer"), Some(Ask::Find { query: "lease".into() }));
    assert_eq!(ask("organize my Videos"), Some(Ask::Organize { folder: "Videos".into(), by: By::Kind }));
    assert_eq!(ask("tidy my downloads by month"), Some(Ask::Organize { folder: "Downloads".into(), by: By::Month }));
    assert_eq!(ask("summarize the files I selected in File Explorer"), Some(Ask::AboutSelection));
    assert_eq!(ask("make the selected photos in explorer smaller"), Some(Ask::Convert { to: None, max: Some(1600) }));
    // A Mac's wording still means what it did.
    assert_eq!(ask("organize my movies"), Some(Ask::Organize { folder: "Movies".into(), by: By::Kind }));
    assert_eq!(folder_name(true, "Movies"), "Videos");
    assert_eq!(folder_name(false, "Videos"), "Movies");
    assert_eq!(folder_name(true, "Downloads"), "Downloads");
}

#[test]
fn a_pcs_files_are_grouped_and_its_own_files_left_alone() {
    assert_eq!(kind_folder_for(true, Path::new("setup.EXE")), "Installers");
    assert_eq!(kind_folder_for(true, Path::new("Win11.iso")), "Installers");
    assert_eq!(kind_folder_for(true, Path::new("app.msix")), "Installers");
    assert_eq!(kind_folder_for(false, Path::new("setup.exe")), "Other", "a Mac does not call a .exe an installer");
    assert_eq!(kind_folder_for(true, Path::new("Zoom.pkg")), "Installers");
    let d = tempfile::tempdir().unwrap();
    for n in ["a.jpg", "setup.exe", "Tools.lnk", "desktop.ini", "Thumbs.db", "~$report.docx", "web.url", "b.pdf", "c.opdownload"] {
        touch(d.path(), n);
    }
    let plan = tidy_plan_for(true, d.path(), By::Kind, now_for_tests()).unwrap();
    let names: Vec<String> = plan.iter().map(|(_, to)| to.strip_prefix(d.path()).unwrap().display().to_string().replace('\\', "/")).collect();
    assert_eq!(names, ["Images/a.jpg", "Documents/b.pdf", "Installers/setup.exe"], "shortcuts and Windows' own files stay");
    // On a Mac the same folder is tidied by the Mac's rules.
    let mac = tidy_plan_for(false, d.path(), By::Kind, now_for_tests()).unwrap();
    assert!(mac.iter().any(|(_, to)| to.ends_with("Other/setup.exe")) && mac.iter().any(|(_, to)| to.ends_with("Other/Tools.lnk")));
}

#[test]
fn on_a_pc_files_in_use_stay_and_the_rest_move() {
    let d = tempfile::tempdir().unwrap();
    let plan: Vec<(PathBuf, PathBuf)> = ["a.pdf", "open.docx", "b.pdf", "denied.pdf"].iter().map(|n| (touch(d.path(), n), d.path().join("Documents").join(n))).collect();
    let rename = |from: &Path, to: &Path| -> std::io::Result<()> {
        match from.file_name().and_then(|n| n.to_str()).unwrap() {
            "open.docx" => Err(std::io::Error::from_raw_os_error(32)),
            "denied.pdf" => Err(std::io::Error::from_raw_os_error(5)),
            _ => std::fs::rename(from, to),
        }
    };
    let (done, in_use, err) = apply_moves_with(true, &plan, &rename);
    assert!(err.is_none(), "{err:?}");
    assert_eq!(done.len(), 2);
    assert_eq!(in_use, ["open.docx", "denied.pdf"]);
    assert!(d.path().join("Documents/a.pdf").exists() && d.path().join("Documents/b.pdf").exists() && d.path().join("open.docx").exists());
    // Any other error stops the tidy, as it does on a Mac, and what was done can be undone.
    let boom = |from: &Path, to: &Path| -> std::io::Result<()> {
        if from.ends_with("c.pdf") {
            Err(std::io::Error::other("disk on fire"))
        } else {
            std::fs::rename(from, to)
        }
    };
    let plan2: Vec<(PathBuf, PathBuf)> = ["c.pdf", "d.pdf"].iter().map(|n| (touch(d.path(), n), d.path().join("More").join(n))).collect();
    let (done, _, err) = apply_moves_with(true, &plan2, &boom);
    assert!(done.is_empty() && err.unwrap().contains("disk on fire"));
    // A Mac never skips: a locked-looking error is an error.
    let (_, in_use, err) = apply_moves_with(false, &plan[1..2], &rename);
    assert!(in_use.is_empty() && err.is_some());
}

#[test]
fn search_results_are_filtered_ranked_and_deduplicated() {
    let out = "name\tC:\\Users\\ada\\.rustup\\toolchains\\x\\tax.html\n\
        name\tC:\\Users\\ada\\AppData\\Local\\Temp\\tax-cache.tmp\n\
        name\tC:\\Users\\ada\\Documents\\syntax-notes.txt\n\
        text\tC:\\Users\\ada\\Documents\\Letters\\accountant.docx\n\
        name\tC:\\Users\\ada\\Documents\\Taxes\\2025 Tax Return.pdf\n\
        name\tC:\\Users\\ada\\documents\\taxes\\2025 tax return.pdf\n\
        name\tC:\\Users\\ada\\Downloads\\tax-form-1040.pdf\n\
        name\tC:\\Users\\ada\\project\\node_modules\\tax\\index.js\n\
        garbage line\n\
        name\t\n";
    let hits = parse_found(out);
    assert_eq!(hits.len(), 8, "the two bad lines are skipped");
    let words = vec!["tax".to_string(), "return".to_string()];
    let home = Path::new("C:\\Users\\ada");
    let ranked = rank_found(hits, &words, home);
    let shown: Vec<String> = ranked.iter().map(|p| p.display().to_string()).collect();
    assert_eq!(
        shown,
        ["C:\\Users\\ada\\Documents\\Taxes\\2025 Tax Return.pdf", "C:\\Users\\ada\\Downloads\\tax-form-1040.pdf", "C:\\Users\\ada\\Documents\\syntax-notes.txt", "C:\\Users\\ada\\Letters\\accountant.docx".replace("\\Letters", "\\Documents\\Letters").as_str()],
        "names with every word first, then names with one of them, then other name matches, then text matches; no app data, hidden folders or dependencies; the same file once"
    );
    // With one word the form is as good a match as the return, and newest-first order decides.
    let one = rank_found(parse_found(out), &["tax".to_string()], home);
    assert!(one[0].display().to_string().ends_with("2025 Tax Return.pdf"), "{one:?}");
    assert!(one[1].display().to_string().ends_with("tax-form-1040.pdf"), "{one:?}");
    for (path, kept) in [
        ("C:\\Users\\ada\\Documents\\a.pdf", true),
        ("C:/Users/ada/OneDrive/Documents/a.pdf", true),
        ("C:\\Users\\ada\\AppData\\Roaming\\a.pdf", false),
        ("C:\\Users\\ada\\.cache\\a.pdf", false),
        ("C:\\Users\\ada\\work\\.git\\config", false),
        ("C:\\Users\\ada\\.hidden.pdf", false),
        ("C:\\Windows\\System32\\a.dll", false),
        ("C:\\Program Files (x86)\\App\\a.txt", false),
        ("C:\\$Recycle.Bin\\S-1\\a.pdf", false),
    ] {
        assert_eq!(keep_found(Path::new(path)), kept, "{path}");
    }
}

#[test]
fn without_windows_search_a_walk_finds_files_by_name() {
    let d = tempfile::tempdir().unwrap();
    for rel in ["Documents/Taxes/2025 Tax Return.pdf", "Documents/tax notes.txt", "Documents/holiday.jpg", "AppData/Local/tax.tmp", ".cache/tax.bin", "node_modules/tax/index.js"] {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, rel).unwrap();
    }
    let found = walk_find(d.path(), &["tax".to_string(), "return".to_string()], std::time::Instant::now() + Duration::from_secs(5));
    let names: Vec<String> = found.iter().map(|(_, p)| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    assert_eq!(names, ["2025 Tax Return.pdf"], "every word, and not from app data, hidden or dependency folders");
    let two = walk_find(d.path(), &["tax".to_string()], std::time::Instant::now() + Duration::from_secs(5));
    assert_eq!(two.len(), 2);
    // An expired deadline stops the walk without an error.
    assert!(walk_find(d.path(), &["tax".to_string()], std::time::Instant::now() - Duration::from_secs(1)).len() <= 1);
}

#[tokio::test]
async fn finding_a_file_on_a_pc_asks_windows_search_and_speaks_in_pc_words() {
    let d = tempfile::tempdir().unwrap();
    let f = PcFiles::default();
    f.found.lock().unwrap().replace(Ok("name\tC:\\Users\\ada\\Documents\\Taxes\\2025 Tax Return.pdf\ntext\tC:\\Users\\ada\\AppData\\x.docx\n".into()));
    let (out, ev) = flow_in("find my tax return pdf", &f, d.path(), None).await;
    let ran = f.ran.lock().unwrap().clone();
    assert_eq!(ran, vec![Command::Win(WinOp::FindFiles { words: vec!["tax".into(), "return".into()], root: d.path().to_path_buf() })]);
    let notes = out.unwrap().1;
    assert!(notes.contains("2025 Tax Return.pdf") && notes.contains("Windows Search") && notes.contains("File Explorer"), "{notes}");
    assert!(!notes.contains("AppData") && !notes.contains("Mac") && !notes.contains("Spotlight") && !notes.contains("Finder"), "{notes}");
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::ToolResult { ok: true, summary, .. } if summary == "1 files found")));
    // Nothing found says so and suggests other words.
    f.found.lock().unwrap().replace(Ok(String::new()));
    let (out, _) = flow_in("find my passport scan pdf", &f, d.path(), None).await;
    assert!(out.unwrap().1.contains("found nothing"));
}

#[tokio::test]
async fn when_windows_search_is_off_the_pc_is_walked_by_file_name() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("Documents/Lease 2026.pdf");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "x").unwrap();
    let f = PcFiles::default(); // the search answers "missing"
    let (out, _) = flow_in("find the lease document on my pc", &f, d.path(), None).await;
    let notes = out.unwrap().1;
    assert!(notes.contains("Lease 2026.pdf") && notes.contains("Windows Search isn't running"), "{notes}");
}

#[tokio::test]
async fn tidying_a_pc_folder_asks_first_moves_loose_files_and_undo_puts_them_back() {
    let d = tempfile::tempdir().unwrap();
    let dl = d.path().join("Downloads");
    std::fs::create_dir_all(&dl).unwrap();
    for n in ["a.jpg", "setup.exe", "b.pdf", "Tools.lnk"] {
        touch(&dl, n);
    }
    let f = PcFiles::default();
    // No: nothing moves.
    let (out, ev) = flow_in("organize my downloads", &f, d.path(), Some(false)).await;
    let ask = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).expect("asked first");
    let text = ask.fields.iter().map(|x| format!("{}: {}", x.label, x.value)).collect::<Vec<_>>().join("\n");
    assert!(text.contains("3 files") && text.contains("Installers: 1") && text.contains("shortcuts") && !text.contains("Finder") && !text.contains("Mac"), "{text}");
    assert_eq!(ask.site, "File Explorer");
    assert!(dl.join("a.jpg").exists() && out.unwrap().1.contains("Nothing was moved"));
    // Yes: they move, the shortcut stays, and Undo is a way back.
    let (out, ev) = flow_in("organize my downloads", &f, d.path(), Some(true)).await;
    assert!(dl.join("Images/a.jpg").exists() && dl.join("Installers/setup.exe").exists() && dl.join("Documents/b.pdf").exists() && dl.join("Tools.lnk").exists());
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(m) = e { Some(m.clone()) } else { None }).expect("a done card");
    assert!(done.ok && done.app == "File Explorer", "{done:?}");
    let notes = out.unwrap().1;
    assert!(notes.contains("Done: BYTE moved 3 files") && !notes.contains("Mac"), "{notes}");
    match macctl::undo_step(&done.undo.expect("an undo token")) {
        Some(Undo::Moves(moves)) => assert_eq!(moves.len(), 3),
        other => panic!("undo should put the moves back, got {other:?}"),
    }
    // A folder that cannot be read says where Windows might be blocking it.
    let (out, _) = flow_in("organize my music", &f, d.path(), Some(true)).await;
    assert!(out.unwrap().1.contains("Controlled folder access"));
    // Videos is a PC's Movies.
    std::fs::create_dir_all(d.path().join("Videos")).unwrap();
    touch(&d.path().join("Videos"), "clip.mp4");
    let (_, ev) = flow_in("organize my movies", &f, d.path(), Some(true)).await;
    assert!(d.path().join("Videos/Video/clip.mp4").exists(), "{ev:?}");
}

#[test]
fn a_pc_saves_photos_in_the_kinds_it_can_write() {
    let files: Vec<PathBuf> = ["C:\\p\\a.HEIC", "C:\\p\\b.jpg", "C:\\p\\c.png", "C:\\p\\d.tiff", "C:\\p\\e.gif", "C:\\p\\notes.txt", "C:\\p\\f.svg"].iter().map(PathBuf::from).collect();
    let show = |plan: Vec<(PathBuf, PathBuf, &'static str)>| plan.into_iter().map(|(_, out, fmt)| format!("{}={fmt}", out.display().to_string().rsplit('\\').next().unwrap())).collect::<Vec<_>>();
    // Only a size: a photo keeps its kind when a PC can write it, and is saved as PNG when not; text and vector files are skipped.
    assert_eq!(show(convert_plan_pc(&files, None, Some(1600))), ["a (small).png=png", "b (small).jpg=jpeg", "c (small).png=png", "d (small).tiff=tiff", "e (small).png=png"]);
    // A kind was asked for.
    assert_eq!(show(convert_plan_pc(&files[..3], Some("jpg"), None)), ["a.jpg=jpeg", "b.jpg=jpeg", "c.jpg=jpeg"]);
    assert_eq!(show(convert_plan_pc(&files[..2], Some("png"), Some(800))), ["a (small).png=png", "b (small).png=png"]);
}

#[tokio::test]
async fn converting_the_explorer_selection_asks_first_keeps_originals_and_can_be_undone() {
    let d = tempfile::tempdir().unwrap();
    let (heic, jpg, txt) = (touch(d.path(), "IMG_1.HEIC"), touch(d.path(), "trip.jpg"), touch(d.path(), "notes.txt"));
    let f = PcFiles::default();
    f.selected.lock().unwrap().replace(Ok(format!("{}\n{}\n{}\n", heic.display(), jpg.display(), txt.display())));
    // No: nothing is made.
    let (out, ev) = flow_in("make the selected photos in explorer smaller", &f, d.path(), Some(false)).await;
    let ask = ev.iter().find_map(|e| if let ChatEvent::Approval(a) = e { Some(a.clone()) } else { None }).expect("asked first");
    let text = ask.fields.iter().map(|x| format!("{}: {}", x.label, x.value)).collect::<Vec<_>>().join("\n");
    assert!(text.contains("IMG_1 (small).png") && text.contains("trip (small).jpg") && !text.contains("notes") && text.contains("Recycle Bin") && !text.contains("Finder"), "{text}");
    assert_eq!(ask.site, "File Explorer");
    assert!(!f.ran.lock().unwrap().iter().any(|c| matches!(c, Command::Win(WinOp::ConvertPhoto { .. }))));
    assert!(out.unwrap().1.contains("Nothing was changed"));
    // Yes: one conversion per photo with an explicit kind and the size limit; the originals stay; Undo removes the copies.
    f.converted.lock().unwrap().extend([Ok("1600x900".into()), Err(RunError::Failed("The image format is not supported.".into()))]);
    let (out, ev) = flow_in("make the selected photos in explorer smaller", &f, d.path(), Some(true)).await;
    let ran = f.ran.lock().unwrap().clone();
    let first = Command::Win(WinOp::ConvertPhoto { src: heic.clone(), out: d.path().join("IMG_1 (small).png"), format: "png".into(), max: Some(1600) });
    let second = Command::Win(WinOp::ConvertPhoto { src: jpg.clone(), out: d.path().join("trip (small).jpg"), format: "jpeg".into(), max: Some(1600) });
    assert!(ran.contains(&first) && ran.contains(&second), "{ran:?}");
    assert!(heic.exists() && jpg.exists(), "originals stay");
    let done = ev.iter().find_map(|e| if let ChatEvent::MacDone(m) = e { Some(m.clone()) } else { None }).expect("a done card");
    assert!(!done.ok && done.app == "File Explorer" && done.detail.starts_with("1 of 2 saved"), "{done:?}");
    let notes = out.unwrap().1;
    assert!(notes.contains("saved 1 new photo files") && notes.contains("trip.jpg: ") && notes.contains("not supported") && !notes.contains("sips"), "{notes}");
    match macctl::undo_step(&done.undo.expect("an undo token")) {
        Some(Undo::Created(files)) => assert_eq!(files, vec![d.path().join("IMG_1 (small).png")]),
        other => panic!("undo should remove the new copies, got {other:?}"),
    }
}

#[tokio::test]
async fn converting_to_a_kind_a_pc_cannot_write_or_with_nothing_selected_changes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let f = PcFiles::default();
    f.selected.lock().unwrap().replace(Ok(touch(d.path(), "a.jpg").display().to_string()));
    let (out, ev) = flow_in("convert the selected photos to heic", &f, d.path(), Some(true)).await;
    assert!(f.ran.lock().unwrap().is_empty() && !ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
    assert!(out.unwrap().1.contains("can't save photos as HEIC"));
    // Nothing selected, or only things that are not photos.
    for sel in ["", "C:\\x\\notes.txt\n"] {
        let f = PcFiles::default();
        f.selected.lock().unwrap().replace(Ok(sel.into()));
        let (out, ev) = flow_in("convert the selected photos to jpg", &f, d.path(), Some(true)).await;
        assert!(!ev.iter().any(|e| matches!(e, ChatEvent::Approval(_))));
        assert!(out.unwrap().1.contains("No photos are selected in File Explorer"));
    }
    // File Explorer that cannot be read says so, in its own name.
    let (out, _) = flow_in("convert the selected photos to jpg", &PcFiles::default(), d.path(), None).await;
    let said = out.unwrap().1;
    assert!(said.contains("couldn't read the File Explorer selection") && !said.contains("Finder"), "{said}");
}

#[tokio::test]
async fn questions_about_the_explorer_selection_read_those_files() {
    let d = tempfile::tempdir().unwrap();
    let note = d.path().join("lease notes.txt");
    std::fs::write(&note, "Rent is 1,200 a month. The lease ends on June 30.").unwrap();
    let f = PcFiles::default();
    f.selected.lock().unwrap().replace(Ok(note.display().to_string()));
    let (out, ev) = flow_in("summarize the files I selected in File Explorer", &f, d.path(), None).await;
    let notes = out.unwrap().1;
    assert!(notes.contains("selected in File Explorer") && notes.contains("Rent is 1,200") && !notes.contains("Finder"), "{notes}");
    assert!(ev.iter().any(|e| matches!(e, ChatEvent::ToolResult { ok: true, .. })));
    // Nothing selected says so.
    f.selected.lock().unwrap().replace(Ok(String::new()));
    let (out, _) = flow_in("summarize the files I selected in File Explorer", &f, d.path(), None).await;
    assert!(out.unwrap().1.contains("Nothing is selected in File Explorer"));
}

/// "Find …" on the real PC, through the whole flow with the real Windows Search. Prints what it found; changes nothing.
#[cfg(windows)]
#[tokio::test]
#[ignore = "asks the real Windows Search; run with --ignored --nocapture"]
async fn live_pc_find_a_file_of_this_pc() {
    for q in ["find my readme file on my pc", "find the notes document on my pc"] {
        let started = std::time::Instant::now();
        let (out, ev) = flow_in(q, &crate::pcctl::WinRunner, &home(), None).await;
        println!("--- {q} ({:.1} s)\n{}", started.elapsed().as_secs_f32(), out.unwrap().1);
        assert!(ev.iter().any(|e| matches!(e, ChatEvent::ToolResult { ok: true, .. })), "{q}");
    }
}
