//! "How's my PC?": a spoken status put together by Rust from the PC's own numbers, never by a model. The wording is a pure
//! function of the numbers, so it is tested from fixtures.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub gpu_temp_c: Option<u32>,
    /// Graphics memory in use and total, bytes.
    pub gpu_mem: Option<(u64, u64)>,
    pub cpu_percent: Option<u32>,
    /// Memory in use and total, bytes.
    pub ram: Option<(u64, u64)>,
    /// The system drive's letter and free bytes.
    pub disk_free: Option<(String, u64)>,
    /// The program using the most memory (all its processes together), and the bytes.
    pub top_app: Option<(String, u64)>,
}

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// "3.4", "12", "2": the number of gigabytes without its unit.
fn gb_number(bytes: u64) -> String {
    let g = bytes as f64 / GB;
    if g >= 10.0 {
        format!("{}", g.round() as u64)
    } else {
        // One decimal, never "3.0".
        let t = (g * 10.0).round() / 10.0;
        if (t - t.round()).abs() < 0.05 {
            format!("{}", t.round() as u64)
        } else {
            format!("{t:.1}")
        }
    }
}

fn gigabytes(bytes: u64) -> String {
    format!("{} gigabytes", gb_number(bytes))
}

/// "6 of 16 gigabytes".
fn gb_of(used: u64, total: u64) -> String {
    format!("{} of {}", gb_number(used), gigabytes(total))
}

/// "chrome.exe" -> "Chrome".
pub fn app_name(raw: &str) -> String {
    let base = raw.trim();
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base);
    let mut c = base.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The sentences, short, in a fixed order. Parts that could not be read are left out; with nothing at all it says so.
pub fn wording(f: &Facts) -> String {
    let mut parts: Vec<String> = Vec::new();
    let hot = |t: u32| if t >= 85 { ", which is hot" } else if t >= 78 { ", a bit warm" } else { "" };
    match (f.gpu_temp_c, f.gpu_mem) {
        (Some(t), Some((used, total))) => parts.push(format!("The graphics card is at {t} degrees{}, using {}.", hot(t), gb_of(used, total))),
        (Some(t), None) => parts.push(format!("The graphics card is at {t} degrees{}.", hot(t))),
        (None, Some((used, total))) => parts.push(format!("The graphics card is using {}.", gb_of(used, total))),
        (None, None) => {}
    }
    if let Some(c) = f.cpu_percent {
        parts.push(format!("The processor is at {c} percent."));
    }
    if let Some((used, total)) = f.ram {
        let pct = if total > 0 { used * 100 / total } else { 0 };
        let tight = if pct >= 90 { ", which is nearly full" } else { "" };
        parts.push(format!("Memory is {} in use{tight}.", gb_of(used, total)));
    }
    if let Some((drive, free)) = &f.disk_free {
        let low = if free / (1024 * 1024 * 1024) < 20 { ", which is low" } else { "" };
        parts.push(format!("Drive {drive} has {} free{low}.", gigabytes(*free)));
    }
    if let Some((name, bytes)) = &f.top_app {
        parts.push(format!("{} is using the most memory, {}.", app_name(name), gigabytes(*bytes)));
    }
    if parts.is_empty() {
        return "I couldn't read anything from this PC right now.".into();
    }
    parts.join(" ")
}

/// `nvidia-smi --query-gpu=temperature.gpu,memory.used,memory.total --format=csv,noheader,nounits` for the busiest card (first line
/// with all three numbers). Memory in MiB.
pub fn parse_nvidia(out: &str) -> Option<(u32, (u64, u64))> {
    out.lines().find_map(|l| {
        let p: Vec<&str> = l.split(',').map(str::trim).collect();
        if p.len() < 3 {
            return None;
        }
        let t = p[0].parse::<u32>().ok()?;
        let used = p[1].parse::<u64>().ok()? * 1024 * 1024;
        let total = p[2].parse::<u64>().ok()? * 1024 * 1024;
        Some((t, (used, total)))
    })
}

fn gpu_facts() -> Option<(u32, (u64, u64))> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=temperature.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    parse_nvidia(&String::from_utf8_lossy(&out.stdout))
}

/// Reads this PC (about a quarter of a second). Blocking.
pub fn gather() -> Facts {
    use sysinfo::{Disks, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(200)));
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let total = sys.total_memory();
    let mut by_app: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for p in sys.processes().values() {
        *by_app.entry(p.name().to_string_lossy().to_lowercase()).or_default() += p.memory();
    }
    let top = by_app.into_iter().filter(|(n, _)| !matches!(n.as_str(), "system" | "memory compression" | "registry" | "[system process]")).max_by_key(|(_, b)| *b);
    let disks = Disks::new_with_refreshed_list();
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let disk = disks
        .list()
        .iter()
        .find(|d| d.mount_point().to_string_lossy().to_uppercase().starts_with(&system_drive.to_uppercase()))
        .or_else(|| disks.list().iter().max_by_key(|d| d.total_space()))
        .map(|d| (d.mount_point().to_string_lossy().trim_end_matches(['\\', '/']).trim_end_matches(':').to_string(), d.available_space()));
    let gpu = gpu_facts();
    Facts {
        gpu_temp_c: gpu.map(|g| g.0),
        gpu_mem: gpu.map(|g| g.1),
        cpu_percent: Some(sys.global_cpu_usage().round() as u32).filter(|_| total > 0),
        ram: (total > 0).then(|| (total.saturating_sub(sys.available_memory()), total)),
        disk_free: disk,
        top_app: top,
    }
}

/// The spoken status of this PC.
pub fn spoken() -> String {
    wording(&gather())
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: u64 = 1024 * 1024 * 1024;

    fn full() -> Facts {
        Facts { gpu_temp_c: Some(54), gpu_mem: Some((6 * G, 16 * G)), cpu_percent: Some(12), ram: Some((18 * G, 32 * G)), disk_free: Some(("C".into(), 210 * G)), top_app: Some(("chrome.exe".into(), 3 * G + G * 4 / 10)) }
    }

    #[test]
    fn the_whole_status_reads_short_and_in_order() {
        assert_eq!(
            wording(&full()),
            "The graphics card is at 54 degrees, using 6 of 16 gigabytes. The processor is at 12 percent. Memory is 18 of 32 gigabytes in use. Drive C has 210 gigabytes free. Chrome is using the most memory, 3.4 gigabytes."
        );
    }

    #[test]
    fn trouble_is_said_plainly() {
        let f = Facts { gpu_temp_c: Some(88), gpu_mem: Some((15 * G, 16 * G)), ram: Some((31 * G, 32 * G)), disk_free: Some(("C".into(), 8 * G)), ..Default::default() };
        let w = wording(&f);
        assert!(w.contains("88 degrees, which is hot"), "{w}");
        assert!(w.contains("nearly full"), "{w}");
        assert!(w.contains("8 gigabytes free, which is low"), "{w}");
        assert!(wording(&Facts { gpu_temp_c: Some(80), ..Default::default() }).contains("a bit warm"));
    }

    #[test]
    fn missing_parts_are_left_out() {
        let f = Facts { cpu_percent: Some(3), ..Default::default() };
        assert_eq!(wording(&f), "The processor is at 3 percent.");
        assert_eq!(wording(&Facts { gpu_temp_c: Some(60), ..Default::default() }), "The graphics card is at 60 degrees.");
        assert_eq!(wording(&Facts::default()), "I couldn't read anything from this PC right now.");
    }

    #[test]
    fn sizes_are_spoken_without_trailing_zeros() {
        assert_eq!(gigabytes(2 * G), "2 gigabytes");
        assert_eq!(gigabytes(G * 25 / 10), "2.5 gigabytes");
        assert_eq!(gigabytes(G * 123 / 10), "12 gigabytes");
        assert_eq!(gigabytes(G / 2), "0.5 gigabytes");
    }

    #[test]
    fn program_names_are_made_readable() {
        assert_eq!(app_name("chrome.exe"), "Chrome");
        assert_eq!(app_name("Code.EXE"), "Code");
        assert_eq!(app_name("explorer"), "Explorer");
    }

    #[test]
    fn nvidia_smi_lines_are_read() {
        assert_eq!(parse_nvidia("54, 6144, 16384\n"), Some((54, (6144 * 1024 * 1024, 16384 * 1024 * 1024))));
        assert_eq!(parse_nvidia("No devices were found"), None);
        assert_eq!(parse_nvidia("[N/A], 1, 2"), None);
    }

    #[test]
    #[ignore = "reads this machine's real numbers; run with --ignored --nocapture"]
    fn live_status_of_this_machine() {
        let w = spoken();
        println!("{w}");
        assert!(!w.is_empty());
    }
}
