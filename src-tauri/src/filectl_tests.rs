use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime};

use super::*;
use crate::engine::Endpoint;
use crate::macctl::RunError;

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
    let out = run_with(&turn, q, fake, SystemTime::now(), &CancellationToken::new(), &send).await.unwrap();
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
