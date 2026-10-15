//! Linux graphics cards: what `gpu::read_adapters` is on Windows (DXGI), read from the kernel and two tools.
//!
//! - The kernel lists every card under `/sys/class/drm/card0`, `card1` and so on, with its PCI vendor and device
//!   id. AMD's driver also says how much memory the card has (`mem_info_vram_total`) and how much system memory
//!   it may borrow (`mem_info_gtt_total`); Intel's discrete cards say it in `lmem_total_bytes`.
//! - NVIDIA's driver says neither in sysfs, so `nvidia-smi` supplies the name and the memory.
//! - `lspci` supplies a readable name for the others ("Radeon RX 7900 XT/7900 XTX/7900M").
//!
//! Everything that decides what a reading MEANS is a pure function over text, tested with outputs copied from
//! real machines. Only `read` touches the system.

use std::path::Path;

use crate::gpu::{classify, Gpu, Vendor};

/// One card as the kernel describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Card {
    pub vendor: u32,
    pub device: u32,
    /// The PCI address, "0000:01:00.0".
    pub slot: String,
    /// Memory that belongs to the card, in bytes, when the driver says.
    pub vram: Option<u64>,
    /// System memory the card may also use, in bytes, when the driver says.
    pub gtt: Option<u64>,
    /// How much of `vram` is in use right now, in bytes, when the driver says (AMD).
    pub used: Option<u64>,
}

const MIB: u64 = 1 << 20;

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn number(s: &str) -> Option<u64> {
    s.trim().parse().ok()
}

fn text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// The cards under a `/sys/class/drm` directory, in order. Connectors ("card0-HDMI-A-1") and render nodes are not cards.
pub fn read_cards(drm: &Path) -> Vec<Card> {
    let Ok(entries) = std::fs::read_dir(drm) else { return Vec::new() };
    let mut named: Vec<(u32, String)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let n: u32 = name.strip_prefix("card")?.parse().ok()?;
            Some((n, name))
        })
        .collect();
    named.sort();
    named
        .into_iter()
        .filter_map(|(_, name)| {
            let dev = drm.join(&name).join("device");
            let vendor = hex(&text(&dev.join("vendor"))?)?;
            let device = hex(&text(&dev.join("device"))?)?;
            let slot = text(&dev.join("uevent"))
                .and_then(|u| u.lines().find_map(|l| l.strip_prefix("PCI_SLOT_NAME=").map(|s| s.trim().to_string())))
                .unwrap_or_default();
            let vram = text(&dev.join("mem_info_vram_total")).and_then(|s| number(&s)).or_else(|| text(&dev.join("lmem_total_bytes")).and_then(|s| number(&s)));
            let gtt = text(&dev.join("mem_info_gtt_total")).and_then(|s| number(&s));
            let used = text(&dev.join("mem_info_vram_used")).and_then(|s| number(&s));
            Some(Card { vendor, device, slot, vram, gtt, used })
        })
        .collect()
}

/// `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`: "NVIDIA GeForce RTX 4060 Ti, 16380".
/// (name, bytes) for each card, in the tool's order.
pub fn parse_nvidia_smi(out: &str) -> Vec<(String, u64)> {
    out.lines()
        .filter_map(|l| {
            let (name, mem) = l.rsplit_once(',')?;
            let mib: u64 = mem.trim().parse().ok()?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_string(), mib * MIB))
        })
        .collect()
}

/// The text inside the last `[...]` of `s`, and `s` without it ("Navi 31 [Radeon RX 7900 XT] [744c]" after the id
/// is removed gives "Radeon RX 7900 XT").
fn bracket(s: &str) -> Option<&str> {
    let end = s.rfind(']')?;
    let start = s[..end].rfind('[')?;
    Some(&s[start + 1..end])
}

/// The words of a device name without its trailing "[id]": ("Navi 31 [Radeon RX 7900 XT] [744c]" → the readable part).
fn device_name(raw: &str) -> String {
    let raw = raw.trim();
    // The last bracket is the PCI id (four hex digits).
    let without_id = match bracket(raw) {
        Some(b) if b.len() == 4 && b.chars().all(|c| c.is_ascii_hexdigit()) => raw[..raw.rfind('[').unwrap_or(raw.len())].trim(),
        _ => raw,
    };
    // Vendors put the marketing name in brackets and a chip codename outside: "AD106 [GeForce RTX 4060 Ti 16GB]".
    bracket(without_id).map(str::to_string).unwrap_or_else(|| without_id.to_string())
}

fn vendor_word(id: u32) -> &'static str {
    match Vendor::from_pci(id) {
        Vendor::Nvidia => "NVIDIA",
        Vendor::Amd => "AMD",
        Vendor::Intel => "Intel",
        Vendor::Other => "",
    }
}

/// `lspci -mm -nn`: (PCI address, readable name) for each display controller. Lines look like
/// `01:00.0 "VGA compatible controller [0300]" "NVIDIA Corporation [10de]" "AD106 [GeForce RTX 4060 Ti 16GB] [2805]" -ra1 ...`.
pub fn parse_lspci(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|line| {
            let (slot, rest) = line.split_once(' ')?;
            let fields: Vec<&str> = rest.split('"').enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, f)| f).collect();
            let class = fields.first()?;
            // Display controllers are PCI class 03xx (VGA 0300, 3D 0302, display 0380).
            let code = bracket(class)?;
            if !code.starts_with("03") {
                return None;
            }
            let vendor_id = bracket(fields.get(1)?).and_then(hex).unwrap_or(0);
            let device = device_name(fields.get(2)?);
            let word = vendor_word(vendor_id);
            let name = if word.is_empty() || device.to_uppercase().contains(&word.to_uppercase()) { device } else { format!("{word} {device}") };
            Some((slot.to_string(), name))
        })
        .collect()
}

/// The machine's graphics cards from what the kernel and the tools said.
pub fn adapters(cards: &[Card], smi: &[(String, u64)], lspci: &[(String, String)]) -> Vec<Gpu> {
    let mut nvidia = smi.iter();
    cards
        .iter()
        .filter_map(|c| {
            let vendor = Vendor::from_pci(c.vendor);
            // lspci writes "01:00.0"; the kernel "0000:01:00.0".
            let described = lspci.iter().find(|(slot, _)| c.slot.ends_with(slot.as_str())).map(|(_, n)| n.clone());
            let (name, dedicated) = match vendor {
                Vendor::Nvidia => match nvidia.next() {
                    Some((n, bytes)) => (n.clone(), *bytes),
                    None => (described.unwrap_or_else(|| "NVIDIA graphics".into()), c.vram.unwrap_or(0)),
                },
                _ => {
                    let fallback = format!("{} graphics", vendor_word(c.vendor)).trim().to_string();
                    (described.unwrap_or(if fallback == "graphics" { "Graphics".into() } else { fallback }), c.vram.unwrap_or(0))
                }
            };
            classify(&name, c.vendor, dedicated, c.gtt.unwrap_or(0), 0)
        })
        .collect()
}

/// The memory `card` has free right now, in bytes. NVIDIA's driver says through `nvidia-smi`, AMD's in sysfs; other
/// vendors do not say, and `None` means "plan from the card's total".
pub fn free_vram(card: &Gpu) -> Option<u64> {
    match card.vendor {
        Vendor::Nvidia => {
            let out = run("nvidia-smi", &["--query-gpu=name,memory.free", "--format=csv,noheader,nounits"])?;
            free_by_name(&parse_nvidia_smi(&out), &card.name)
        }
        Vendor::Amd => free_amd(&read_cards(Path::new("/sys/class/drm")), card.dedicated_bytes),
        _ => None,
    }
}

/// `nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits`: "3314055, 5414" per program using the
/// card. (pid, bytes).
pub fn parse_compute_apps(out: &str) -> Vec<(u32, u64)> {
    out.lines()
        .filter_map(|l| {
            let (pid, mib) = l.split_once(',')?;
            Some((pid.trim().parse().ok()?, mib.trim().parse::<u64>().ok()? * MIB))
        })
        .collect()
}

/// What the programs with these process ids hold on NVIDIA cards, in bytes. `None` when the tool is not there.
pub fn used_by(pids: &[u32]) -> Option<u64> {
    let out = run("nvidia-smi", &["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"])?;
    Some(parse_compute_apps(&out).iter().filter(|(p, _)| pids.contains(p)).map(|(_, b)| *b).sum())
}

/// The free memory of the NVIDIA card called `name` among `nvidia-smi`'s (name, free bytes) rows. Two cards of one
/// model are indistinguishable by name; the first is taken, which is the one the engine uses first.
pub fn free_by_name(rows: &[(String, u64)], name: &str) -> Option<u64> {
    rows.iter().find(|(n, _)| n == name).map(|(_, free)| *free)
}

/// The free memory of the AMD card with `total` bytes of its own, from what the driver says is in use.
pub fn free_amd(cards: &[Card], total: u64) -> Option<u64> {
    let c = cards.iter().find(|c| Vendor::from_pci(c.vendor) == Vendor::Amd && c.vram == Some(total))?;
    Some(total.saturating_sub(c.used?))
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Reads this machine's cards.
pub fn read() -> Vec<Gpu> {
    let cards = read_cards(Path::new("/sys/class/drm"));
    if cards.is_empty() {
        return Vec::new();
    }
    let smi = if cards.iter().any(|c| Vendor::from_pci(c.vendor) == Vendor::Nvidia) {
        run("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).map(|o| parse_nvidia_smi(&o)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let lspci = run("lspci", &["-mm", "-nn"]).map(|o| parse_lspci(&o)).unwrap_or_default();
    adapters(&cards, &smi, &lspci)
}

/// Whether the Vulkan loader is installed. It comes with the graphics driver packages, so a machine with no GPU
/// driver (a server, a container) lacks it, and an engine linked against it cannot start. The engine loads
/// `libvulkan.so.1`, so that is what is looked for, in the places the system keeps libraries.
pub fn vulkan_loader_present() -> bool {
    crate::syslib::present(&["libvulkan.so.1"])
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    // Copied from machines: `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`.
    const SMI_4060TI: &str = "NVIDIA GeForce RTX 4060 Ti, 16380\n";
    const SMI_TWO: &str = "NVIDIA GeForce RTX 4090, 24564\nNVIDIA GeForce RTX 3060, 12288\n";

    // `lspci -mm -nn`, display lines only plus a few that must be ignored.
    const LSPCI_NVIDIA_AMD_IGPU: &str = r#"00:00.0 "Host bridge [0600]" "Advanced Micro Devices, Inc. [AMD] [1022]" "Device [14d8]" -p00 "ASRock Incorporation [1849]" "Device [14d8]"
01:00.0 "VGA compatible controller [0300]" "NVIDIA Corporation [10de]" "AD106 [GeForce RTX 4060 Ti 16GB] [2805]" -ra1 -p00 "ASUSTeK Computer Inc. [1043]" "Device [88f5]"
01:00.1 "Audio device [0403]" "NVIDIA Corporation [10de]" "AD106M High Definition Audio Controller [22bd]" -ra1 -p00 "ASUSTeK Computer Inc. [1043]" "Device [88f5]"
0f:00.0 "Display controller [0380]" "Advanced Micro Devices, Inc. [AMD/ATI] [1002]" "Raphael [164e]" -rc1 -p00 "ASRock Incorporation [1849]" "Device [164e]"
"#;
    const LSPCI_RADEON: &str = r#"0b:00.0 "VGA compatible controller [0300]" "Advanced Micro Devices, Inc. [AMD/ATI] [1002]" "Navi 31 [Radeon RX 7900 XT/7900 XTX/7900M] [744c]" -rc8 -p00 "Sapphire Technology Limited [1da2]" "Device [e471]"
"#;
    const LSPCI_INTEL: &str = r#"00:02.0 "VGA compatible controller [0300]" "Intel Corporation [8086]" "Raptor Lake-S GT1 [UHD Graphics 770] [a780]" -rc1 -p00 "Gigabyte Technology Co., Ltd [1458]" "Device [d000]"
03:00.0 "VGA compatible controller [0300]" "Intel Corporation [8086]" "DG2 [Arc A770] [56a0]" -rc8 -p00 "Intel Corporation [8086]" "Device [1020]"
"#;

    fn card(vendor: u32, slot: &str, vram: Option<u64>, gtt: Option<u64>) -> Card {
        Card { vendor, device: 0, slot: slot.into(), vram, gtt, used: None }
    }

    #[test]
    fn the_memory_a_program_holds_comes_from_the_compute_list() {
        let out = "14596, 10056\n3309389, 218\n3314055, 5414\n";
        let apps = parse_compute_apps(out);
        assert_eq!(apps, vec![(14596, 10056 * MIB), (3309389, 218 * MIB), (3314055, 5414 * MIB)]);
        // Only the programs asked about are counted.
        let held: u64 = apps.iter().filter(|(p, _)| [3314055u32].contains(p)).map(|(_, b)| *b).sum();
        assert_eq!(held, 5414 * MIB);
        assert!(parse_compute_apps("").is_empty());
        assert!(parse_compute_apps("No running processes found\n").is_empty());
    }

    #[test]
    fn free_memory_comes_from_the_tool_for_nvidia_and_the_driver_for_amd() {
        let rows = parse_nvidia_smi("NVIDIA GeForce RTX 4060 Ti, 5407\nNVIDIA GeForce RTX 3060, 11000\n");
        assert_eq!(free_by_name(&rows, "NVIDIA GeForce RTX 4060 Ti"), Some(5407 * MIB));
        assert_eq!(free_by_name(&rows, "NVIDIA GeForce RTX 3060"), Some(11000 * MIB));
        assert_eq!(free_by_name(&rows, "NVIDIA GeForce RTX 4090"), None);
        let mut amd = card(0x1002, "0000:0b:00.0", Some(20 * GIB), None);
        amd.used = Some(4 * GIB);
        assert_eq!(free_amd(&[amd.clone()], 20 * GIB), Some(16 * GIB));
        // A driver that does not say how much is used gives no answer, never a guess.
        amd.used = None;
        assert_eq!(free_amd(&[amd], 20 * GIB), None);
        assert_eq!(free_amd(&[], 20 * GIB), None);
    }

    #[test]
    fn nvidia_smi_gives_the_name_and_the_memory() {
        assert_eq!(parse_nvidia_smi(SMI_4060TI), vec![("NVIDIA GeForce RTX 4060 Ti".to_string(), 16380 * MIB)]);
        assert_eq!(parse_nvidia_smi(SMI_TWO).len(), 2);
        assert!(parse_nvidia_smi("No devices were found\n").is_empty());
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn lspci_lists_only_display_controllers_with_readable_names() {
        let l = parse_lspci(LSPCI_NVIDIA_AMD_IGPU);
        assert_eq!(l, vec![("01:00.0".to_string(), "NVIDIA GeForce RTX 4060 Ti 16GB".to_string()), ("0f:00.0".to_string(), "AMD Raphael".to_string())]);
        assert_eq!(parse_lspci(LSPCI_RADEON)[0].1, "AMD Radeon RX 7900 XT/7900 XTX/7900M");
        let i = parse_lspci(LSPCI_INTEL);
        assert_eq!(i[0].1, "Intel UHD Graphics 770");
        assert_eq!(i[1].1, "Intel Arc A770");
    }

    #[test]
    fn a_desktop_with_an_nvidia_card_and_amd_graphics_in_the_cpu() {
        let cards = [card(0x10DE, "0000:01:00.0", None, None), card(0x1002, "0000:0f:00.0", Some(512 * MIB), Some(16 * GIB))];
        let gpus = adapters(&cards, &parse_nvidia_smi(SMI_4060TI), &parse_lspci(LSPCI_NVIDIA_AMD_IGPU));
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4060 Ti");
        assert_eq!(gpus[0].vendor, Vendor::Nvidia);
        assert_eq!(gpus[0].dedicated_bytes, 16380 * MIB);
        assert!(!gpus[0].integrated);
        assert_eq!(gpus[1].name, "AMD Raphael");
        assert!(gpus[1].integrated, "a 512 MB carve-out is integrated graphics");
        assert_eq!(gpus[1].shared_bytes, 16 * GIB);
        // The engine runs on the discrete card.
        assert_eq!(crate::gpu::best_discrete(&gpus).map(|g| g.name.as_str()), Some("NVIDIA GeForce RTX 4060 Ti"));
        assert_eq!(crate::gpu::backend_for(&gpus), if cfg!(target_os = "macos") { crate::chip::Backend::Metal } else { crate::chip::Backend::Cuda });
    }

    #[test]
    fn an_amd_card_reports_its_own_memory_and_runs_on_vulkan() {
        let gpus = adapters(&[card(0x1002, "0000:0b:00.0", Some(20 * GIB), Some(32 * GIB))], &[], &parse_lspci(LSPCI_RADEON));
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].dedicated_bytes, 20 * GIB);
        assert!(!gpus[0].integrated);
        // The speed table finds it by the model in the name.
        assert!(crate::gpu::profile(&gpus[0].name).is_some(), "{}", gpus[0].name);
        if !cfg!(target_os = "macos") {
            assert_eq!(crate::gpu::backend_for(&gpus), crate::chip::Backend::Vulkan);
        }
    }

    #[test]
    fn laptop_graphics_in_the_processor_and_a_discrete_arc_card() {
        let cards = [card(0x8086, "0000:00:02.0", None, None), card(0x8086, "0000:03:00.0", Some(16 * GIB), None)];
        let gpus = adapters(&cards, &[], &parse_lspci(LSPCI_INTEL));
        assert_eq!(gpus.len(), 2);
        // The processor's graphics report no memory of their own.
        assert!(gpus[0].integrated && gpus[0].dedicated_bytes == 0);
        assert!(!gpus[1].integrated && gpus[1].name == "Intel Arc A770");
    }

    #[test]
    fn two_nvidia_cards_take_the_tools_answers_in_order() {
        let cards = [card(0x10DE, "0000:01:00.0", None, None), card(0x10DE, "0000:02:00.0", None, None)];
        let gpus = adapters(&cards, &parse_nvidia_smi(SMI_TWO), &[]);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(gpus[1].name, "NVIDIA GeForce RTX 3060");
        assert_eq!(crate::gpu::best_discrete(&gpus).map(|g| g.name.as_str()), Some("NVIDIA GeForce RTX 4090"));
    }

    #[test]
    fn virtual_and_unknown_adapters_are_not_offered_as_gpus() {
        // A virtual machine's display adapter has no memory and an unknown vendor.
        assert!(adapters(&[card(0x1AF4, "0000:00:01.0", None, None)], &[], &[]).is_empty());
        assert!(adapters(&[], &[], &[]).is_empty());
    }

    #[test]
    fn an_nvidia_card_without_the_vendor_tool_is_still_listed() {
        let gpus = adapters(&[card(0x10DE, "0000:01:00.0", None, None)], &[], &parse_lspci(LSPCI_NVIDIA_AMD_IGPU));
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4060 Ti 16GB");
    }

    #[test]
    fn the_cards_are_read_from_a_sysfs_tree() {
        let dir = tempfile::tempdir().unwrap();
        let put = |rel: &str, body: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        put("card1/device/vendor", "0x1002\n");
        put("card1/device/device", "0x744c\n");
        put("card1/device/uevent", "DRIVER=amdgpu\nPCI_SLOT_NAME=0000:0b:00.0\n");
        put("card1/device/mem_info_vram_total", "21458059264\n");
        put("card1/device/mem_info_gtt_total", "33554432000\n");
        put("card0/device/vendor", "0x10de\n");
        put("card0/device/device", "0x2805\n");
        put("card0/device/uevent", "DRIVER=nvidia\nPCI_SLOT_NAME=0000:01:00.0\n");
        // Connectors and render nodes are not cards.
        put("card0-HDMI-A-1/status", "connected\n");
        put("renderD128/dev", "226:128\n");
        let cards = read_cards(dir.path());
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[0].vendor, cards[0].slot.as_str(), cards[0].vram), (0x10DE, "0000:01:00.0", None));
        assert_eq!((cards[1].vendor, cards[1].vram, cards[1].gtt), (0x1002, Some(21458059264), Some(33554432000)));
        assert!(read_cards(&dir.path().join("missing")).is_empty());
    }
}
