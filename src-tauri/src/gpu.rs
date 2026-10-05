//! The graphics hardware in a PC: which cards there are, who made them, and how
//! much memory each has.
//!
//! A Mac has one pool of memory shared by CPU and GPU, so "how big a model fits" is
//! a share of RAM and `chip.rs` can look the chip up by name. A PC is the opposite
//! case: a discrete card has its OWN fixed memory, unrelated to system RAM. Treating
//! it as unified made the planner offer 21 GB to a 16 GB card, and until this module
//! existed nothing on Windows told the planner what the card actually was.
//!
//! Detection is DXGI on Windows. It is used rather than WMI because WMI's
//! `AdapterRAM` is a 32-bit field and reports a 16 GB card as 4 GB (measured: 4095 MB for a
//! 16 GB card), while DXGI's `DedicatedVideoMemory` is the real
//! figure.
//!
//! Everything that decides what a reading MEANS is a pure function, so it is tested
//! with fixtures of machines nobody has to own.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Vendor {
    /// From the PCI vendor id every adapter reports.
    pub fn from_pci(id: u32) -> Vendor {
        match id {
            0x10DE => Vendor::Nvidia,
            0x1002 | 0x1022 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            _ => Vendor::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    pub vendor: Vendor,
    /// Memory that belongs to the card alone.
    pub dedicated_bytes: u64,
    /// System memory the GPU may also use: an integrated GPU's real memory.
    pub shared_bytes: u64,
    /// Shares system memory instead of having its own (a laptop or CPU graphics).
    pub integrated: bool,
}

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// DXGI_ADAPTER_FLAG_SOFTWARE: the Microsoft Basic Render Driver and the like.
const FLAG_SOFTWARE: u32 = 2;

/// What one adapter reading is, or None when it is not a GPU worth listing.
///
/// Skipped on purpose: software renderers, and zero-memory virtual adapters. A
/// remote-desktop tool's virtual display adapter has no memory at all; listing it would
/// offer the planner a "GPU" that cannot hold a single weight.
pub fn classify(name: &str, vendor_id: u32, dedicated: u64, shared: u64, flags: u32) -> Option<Gpu> {
    if flags & FLAG_SOFTWARE != 0 {
        return None;
    }
    let vendor = Vendor::from_pci(vendor_id);
    if vendor == Vendor::Other && dedicated == 0 {
        return None;
    }
    // An integrated GPU reports a small carve-out (AMD's integrated graphics says
    // 512 MB) and borrows system RAM for the rest. Nothing a model could use is that small.
    let integrated = dedicated < 1536 * MIB;
    Some(Gpu { name: name.trim().to_string(), vendor, dedicated_bytes: dedicated, shared_bytes: shared, integrated })
}

/// The card to run a model on: the discrete GPU with the most memory.
pub fn best_discrete(gpus: &[Gpu]) -> Option<&Gpu> {
    gpus.iter().filter(|g| !g.integrated).max_by_key(|g| g.dedicated_bytes)
}

/// Which engine build should serve the model.
///
/// CUDA only on NVIDIA, where it is the fastest engine: measured on a 16 GB NVIDIA card it
/// reads a prompt 14% faster than Vulkan (12,600 against 11,090 tokens/s) and generates
/// 10% faster (213 against 194). That margin is what the CUDA build's size buys; whether it is
/// worth it is a packaging decision (see docs/WINDOWS-PACKAGING.md), not a speed necessity.
/// Every other vendor, and integrated graphics, goes through Vulkan; with no GPU at all the
/// CPU path is used.
pub fn backend_for(gpus: &[Gpu]) -> crate::chip::Backend {
    use crate::chip::Backend;
    if cfg!(target_os = "macos") {
        return Backend::Metal;
    }
    match best_discrete(gpus) {
        Some(g) if g.vendor == Vendor::Nvidia && cuda_can_run(&g.name) => Backend::Cuda,
        Some(_) => Backend::Vulkan,
        None if !gpus.is_empty() => Backend::Vulkan,
        None => Backend::Cpu,
    }
}

/// Whether the bundled CUDA build can run on this NVIDIA card. It is compiled for the Turing
/// generation (RTX 20 series, GTX 16 series) and newer, which is as far back as CUDA 13 goes:
/// older cards (GTX 900 and 10 series, the Titan X, older Quadro and Tesla, the MX150 to MX350)
/// have no kernels in it and no PTX to fall back on, so the engine would start and then find
/// nothing to run on. They are sent to the Vulkan engine instead, which every such card's
/// driver supports. Only names that are known to be older are refused; an unknown card gets
/// CUDA, as before.
pub fn cuda_can_run(name: &str) -> bool {
    let n = name.to_uppercase();
    const OLDER: &[&str] = &[
        "GTX 9", "GTX 10", "GTX 8", "GTX 7", "GTX 6", "GTX 5", "GTX 4", "GTX TITAN", "TITAN X", "TITAN V",
        "GT 10", "GT 9", "GT 7", "GT 6", "GT 5", "GT 4",
        "QUADRO P", "QUADRO M", "QUADRO K", "QUADRO GP100", "QUADRO GV100", "TESLA P", "TESLA K", "TESLA M", "TESLA V100",
        "MX1", "MX2", "MX3",
    ];
    !OLDER.iter().any(|p| n.contains(p))
}

/// Whether the Vulkan loader is installed. vulkan-1.dll comes with a graphics DRIVER, not
/// with Windows, so a machine with no GPU driver (a VM, a basic display adapter) lacks it,
/// and an engine linked against it cannot even start. Checked by loading it, which is also
/// exactly what the engine will do.
#[cfg(windows)]
fn vulkan_loader_present() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    match unsafe { LoadLibraryW(w!("vulkan-1.dll")) } {
        Ok(h) => {
            let _ = unsafe { FreeLibrary(h) };
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn vulkan_loader_present() -> bool {
    true
}

/// The backend that will actually run: `backend_for`'s answer, unless it is Vulkan on a
/// machine that cannot load Vulkan, in which case the model runs on the CPU through the
/// other build. Choosing Vulkan there would leave the person with no engine at all.
pub fn effective_backend() -> crate::chip::Backend {
    use crate::chip::Backend;
    static B: std::sync::OnceLock<Backend> = std::sync::OnceLock::new();
    *B.get_or_init(|| {
        if runs_on_cpu_only(cfg!(windows), cfg!(target_arch = "aarch64")) {
            return Backend::Cpu;
        }
        match backend_for(detect()) {
            Backend::Vulkan if !vulkan_loader_present() => Backend::Cpu,
            b => b,
        }
    })
}

/// The Windows build for ARM64 (Snapdragon laptops) carries the CPU engine only: the CUDA and
/// Vulkan builds are x64, so a graphics chip it finds is never what runs the model. Saying
/// Vulkan there would also make speed estimates assume a GPU that is not being used.
pub fn runs_on_cpu_only(windows: bool, arm64: bool) -> bool {
    windows && arm64
}

/// Which bundled engine binary serves a backend. The CUDA build is the default and also runs
/// on the CPU, with the runtime it needs bundled beside it; the Vulkan build exists only for
/// Windows, where it is the one that covers AMD and Intel.
pub fn engine_sidecar(backend: crate::chip::Backend) -> &'static str {
    match backend {
        crate::chip::Backend::Vulkan if cfg!(windows) => "llama-server-vulkan",
        _ => "llama-server",
    }
}

/// (memory bandwidth in GB/s, FP16 TFLOPS), approximate and from published specs. They
/// seed the FIRST speed estimate a user sees; BYTE's own tuning measures the real machine
/// and replaces them, which is why they are not presented as exact.
const TABLE: &[(&str, f64, f64)] = &[
    // NVIDIA RTX 50
    ("RTX 5090", 1792.0, 210.0), ("RTX 5080", 960.0, 112.0), ("RTX 5070 TI", 896.0, 88.0), ("RTX 5070", 672.0, 62.0),
    ("RTX 5060 TI", 448.0, 48.0), ("RTX 5060", 448.0, 38.0),
    // NVIDIA RTX 40
    ("RTX 4090", 1008.0, 165.0), ("RTX 4080 SUPER", 736.0, 104.0), ("RTX 4080", 717.0, 97.0), ("RTX 4070 TI SUPER", 672.0, 88.0),
    ("RTX 4070 TI", 504.0, 80.0), ("RTX 4070 SUPER", 504.0, 71.0), ("RTX 4070", 504.0, 58.0), ("RTX 4060 TI", 288.0, 44.0),
    ("RTX 4060", 272.0, 30.0),
    // NVIDIA RTX 30
    ("RTX 3090 TI", 1008.0, 80.0), ("RTX 3090", 936.0, 71.0), ("RTX 3080 TI", 912.0, 68.0), ("RTX 3080", 760.0, 59.0),
    ("RTX 3070 TI", 608.0, 44.0), ("RTX 3070", 448.0, 40.0), ("RTX 3060 TI", 448.0, 32.0), ("RTX 3060", 360.0, 25.0),
    // NVIDIA RTX 20 and GTX
    ("RTX 2080 TI", 616.0, 57.0), ("RTX 2080 SUPER", 496.0, 45.0), ("RTX 2080", 448.0, 40.0), ("RTX 2070 SUPER", 448.0, 36.0),
    ("RTX 2070", 448.0, 30.0), ("RTX 2060 SUPER", 448.0, 29.0), ("RTX 2060", 336.0, 26.0),
    ("GTX 1660 SUPER", 336.0, 5.0), ("GTX 1660 TI", 288.0, 5.5), ("GTX 1660", 192.0, 5.0), ("GTX 1650", 128.0, 3.0),
    ("GTX 1080 TI", 484.0, 11.0), ("GTX 1080", 320.0, 9.0), ("GTX 1070 TI", 256.0, 8.0), ("GTX 1070", 256.0, 6.5), ("GTX 1060", 192.0, 4.4),
    // AMD integrated graphics under the generic name Windows gives the 2-compute-unit chips on
    // desktop Ryzen 7000 and similar. MEASURED, not published: Qwen3 0.6B Q8_0 through the Vulkan
    // engine on the AMD Windows driver generated 19-27 tok/s and read a 2,781-token prompt at
    // 232 tok/s. Those correspond to about 20 GB/s and 0.35 TFLOPS. The 50 GB/s and 4 TFLOPS
    // guess for unknown integrated graphics promised about twice the generation speed and
    // twelve times the prompt speed of this chip. Larger integrated GPUs name themselves
    // ("780M") and are not matched by this entry.
    ("AMD RADEON(TM) GRAPHICS", 20.0, 0.35),
    // AMD
    ("RX 7900 XTX", 960.0, 123.0), ("RX 7900 XT", 800.0, 103.0), ("RX 7900 GRE", 576.0, 92.0), ("RX 7800 XT", 624.0, 74.0),
    ("RX 7700 XT", 432.0, 70.0), ("RX 7600 XT", 288.0, 45.0), ("RX 7600", 288.0, 43.0),
    ("RX 6950 XT", 576.0, 47.0), ("RX 6900 XT", 512.0, 46.0), ("RX 6800 XT", 512.0, 41.0), ("RX 6800", 512.0, 34.0),
    ("RX 6750 XT", 432.0, 26.0), ("RX 6700 XT", 384.0, 26.0), ("RX 6600 XT", 256.0, 21.0), ("RX 6600", 224.0, 18.0),
    // Intel Arc
    ("ARC B580", 456.0, 55.0), ("ARC B570", 380.0, 46.0), ("ARC A770", 560.0, 39.0), ("ARC A750", 512.0, 34.0),
    ("ARC A580", 512.0, 25.0), ("ARC A380", 186.0, 10.0),
];

/// The table's figures for a card, matched by the LONGEST model name found in its
/// adapter name, so "RTX 4070 Ti SUPER" never falls back to "RTX 4070".
pub fn profile(name: &str) -> Option<(f64, f64)> {
    let n = name.to_uppercase();
    TABLE.iter().filter(|(k, _, _)| n.contains(k)).max_by_key(|(k, _, _)| k.len()).map(|(_, bw, tf)| (*bw, *tf))
}

/// A guess for a card the table does not know, from what DXGI does report. Deliberately
/// modest: an unknown card promised too much costs the user's trust, too little costs a
/// mild surprise.
pub fn guess(gpu: &Gpu) -> (f64, f64) {
    if gpu.integrated {
        // Shares system memory, so it is as fast as that memory is.
        (50.0, 4.0)
    } else {
        let gib = gpu.dedicated_bytes as f64 / GIB as f64;
        ((gib * 25.0).clamp(100.0, 800.0), (gib * 4.0).clamp(4.0, 100.0))
    }
}

#[cfg(windows)]
fn read_adapters() -> Vec<Gpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else { continue };
        // One physical card can be listed twice under different LUIDs: one machine shows
        // its NVIDIA card twice, identical in vendor, device, subsystem, revision and memory
        // (nvidia-smi and the engine both see one card). DXGI cannot tell that from two
        // genuinely identical cards, so identical descriptors collapse to one. That is the
        // safe direction: the planner budgets from the single best card either way. Real
        // multi-GPU support should ask the engine, which enumerates its own devices.
        if !seen.insert((d.VendorId, d.DeviceId, d.SubSysId, d.Revision, d.DedicatedVideoMemory)) {
            continue;
        }
        let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        let name = String::from_utf16_lossy(&d.Description[..len]);
        if let Some(g) = classify(&name, d.VendorId, d.DedicatedVideoMemory as u64, d.SharedSystemMemory as u64, d.Flags) {
            out.push(g);
        }
    }
    out
}

#[cfg(not(windows))]
fn read_adapters() -> Vec<Gpu> {
    Vec::new()
}

/// The machine's GPUs, read once (a few milliseconds, and cards do not change while
/// BYTE runs). Empty on a Mac, whose memory is unified and handled by chip.rs.
pub fn detect() -> &'static [Gpu] {
    static GPUS: std::sync::OnceLock<Vec<Gpu>> = std::sync::OnceLock::new();
    GPUS.get_or_init(read_adapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arm64_windows_build_runs_on_the_cpu_whatever_graphics_it_finds() {
        assert!(runs_on_cpu_only(true, true));
        // x64 Windows, and every Mac and Linux machine, choose by hardware as before.
        assert!(!runs_on_cpu_only(true, false));
        assert!(!runs_on_cpu_only(false, true));
        assert!(!runs_on_cpu_only(false, false));
    }

    const NVIDIA: u32 = 0x10DE;
    const AMD: u32 = 0x1002;
    const INTEL: u32 = 0x8086;

    /// A hybrid PC's adapters as Windows lists them, plus the junk it lists beside them.
    #[test]
    fn a_hybrid_pc_is_read_the_way_it_really_is() {
        let rtx = classify("NVIDIA GeForce RTX 4070", NVIDIA, 12 * GIB, 16 * GIB, 0).unwrap();
        assert!(!rtx.integrated && rtx.vendor == Vendor::Nvidia);
        let igpu = classify("AMD Radeon(TM) Graphics", AMD, 512 * MIB, 16 * GIB, 0).unwrap();
        assert!(igpu.integrated, "a 512 MB carve-out is integrated graphics");
        assert!(classify("Virtual Display Adapter", 0x1AE0, 0, 0, 0).is_none(), "a zero-memory virtual adapter is not a GPU");
        assert!(classify("Microsoft Basic Render Driver", 0x1414, 0, 0, FLAG_SOFTWARE).is_none(), "a software renderer is not a GPU");
        let all = [rtx.clone(), igpu];
        assert_eq!(best_discrete(&all).unwrap().name, rtx.name, "the discrete card wins over the integrated one");
    }

    #[test]
    fn the_engine_follows_the_hardware() {
        use crate::chip::Backend;
        let nv = classify("RTX 4060", NVIDIA, 8 * GIB, 0, 0).unwrap();
        let amd = classify("RX 7900 GRE", AMD, 16 * GIB, 0, 0).unwrap();
        let arc = classify("Arc A770", INTEL, 16 * GIB, 0, 0).unwrap();
        let ig = classify("Intel UHD Graphics", INTEL, 128 * MIB, 8 * GIB, 0).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(backend_for(&[nv]), Backend::Metal);
            return;
        }
        assert_eq!(backend_for(&[nv.clone()]), Backend::Cuda, "NVIDIA gets CUDA");
        assert_eq!(backend_for(&[amd]), Backend::Vulkan);
        assert_eq!(backend_for(&[arc]), Backend::Vulkan);
        assert_eq!(backend_for(&[ig.clone()]), Backend::Vulkan, "integrated graphics still beats the CPU");
        assert_eq!(backend_for(&[]), Backend::Cpu, "no GPU at all");
        // A discrete NVIDIA card beside an integrated GPU still uses CUDA.
        assert_eq!(backend_for(&[ig, nv]), Backend::Cuda);
    }

    #[test]
    fn older_nvidia_cards_get_the_engine_that_can_run_on_them() {
        use crate::chip::Backend;
        for old in ["NVIDIA GeForce GTX 1060", "GeForce GTX 1080 Ti", "GeForce GTX 980 Ti", "GeForce GTX 750 Ti", "TITAN X (Pascal)", "TITAN Xp", "TITAN V",
                    "Quadro P2000", "Quadro M4000", "Tesla P100", "Tesla V100", "GeForce MX250", "GeForce MX150", "GeForce GT 1030", "GeForce GTX TITAN Black"] {
            assert!(!cuda_can_run(old), "{old} is older than the CUDA build supports");
        }
        for new in ["NVIDIA GeForce GTX 1650", "GeForce GTX 1660 SUPER", "GeForce RTX 2060", "GeForce RTX 3050", "GeForce RTX 4060 Ti", "GeForce RTX 5080",
                    "TITAN RTX", "Tesla T4", "A100-SXM4-40GB", "NVIDIA L4", "Quadro RTX 4000", "RTX A2000", "GeForce MX450", "Some Future NVIDIA Card"] {
            assert!(cuda_can_run(new), "{new} is Turing or newer, or unknown");
        }
        if cfg!(target_os = "macos") {
            return;
        }
        let pascal = classify("GeForce GTX 1060", NVIDIA, 6 * GIB, 0, 0).unwrap();
        let turing = classify("GeForce GTX 1650", NVIDIA, 4 * GIB, 0, 0).unwrap();
        assert_eq!(backend_for(&[pascal]), Backend::Vulkan, "no CUDA kernels exist for it");
        assert_eq!(backend_for(&[turing]), Backend::Cuda);
    }

    #[test]
    fn each_backend_has_the_engine_binary_that_suits_it() {
        use crate::chip::Backend;
        assert_eq!(engine_sidecar(Backend::Cuda), "llama-server");
        assert_eq!(engine_sidecar(Backend::Metal), "llama-server");
        assert_eq!(engine_sidecar(Backend::Cpu), "llama-server", "no GPU runs on the build that bundles its own runtime");
        let vk = engine_sidecar(Backend::Vulkan);
        assert_eq!(vk, if cfg!(windows) { "llama-server-vulkan" } else { "llama-server" });
    }

    #[test]
    fn the_effective_backend_is_reported() {
        eprintln!("effective backend: {:?} -> {}", effective_backend(), engine_sidecar(effective_backend()));
    }

    #[test]
    fn the_longest_model_name_wins() {
        assert_eq!(profile("NVIDIA GeForce RTX 4070 Ti SUPER").unwrap().0, 672.0, "Ti SUPER, not plain 4070");
        assert_eq!(profile("NVIDIA GeForce RTX 4070 Ti").unwrap().0, 504.0);
        assert_eq!(profile("AMD Radeon RX 7900 GRE").unwrap().0, 576.0);
        assert_eq!(profile("NVIDIA GeForce RTX 5070 Ti").unwrap().0, 896.0);
        assert!(profile("Some Future GPU 9000").is_none(), "an unknown card is not invented");
        // The generic name of the small AMD integrated chips is measured, and a named one is not caught by it.
        assert_eq!(profile("AMD Radeon(TM) Graphics").unwrap(), (20.0, 0.35));
        assert!(profile("AMD Radeon(TM) 780M Graphics").is_none(), "a larger integrated GPU is not this chip");
    }

    #[test]
    fn an_unknown_card_gets_a_modest_guess_not_a_flattering_one() {
        let unknown = classify("Mystery GPU", NVIDIA, 8 * GIB, 0, 0).unwrap();
        let (bw, _) = guess(&unknown);
        assert!(bw <= 300.0, "{bw} GB/s is more than an unknown 8 GB card should be promised");
        let ig = classify("Mystery iGPU", AMD, 256 * MIB, 8 * GIB, 0).unwrap();
        assert!(guess(&ig).0 <= 60.0, "integrated graphics is only as fast as system memory");
    }

    #[test]
    fn this_machine_can_be_read_without_panicking() {
        // Informational: a runner with no GPU legitimately returns an empty list.
        eprintln!("detected GPUs: {:#?}", detect());
    }

    /// The raw adapter descriptors, field by field, for working out what distinguishes
    /// two adapters that look the same. Informational; prints and passes.
    #[cfg(windows)]
    #[test]
    fn raw_adapter_descriptors() {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.unwrap();
        let mut i = 0;
        while let Ok(a) = unsafe { factory.EnumAdapters1(i) } {
            let d = unsafe { a.GetDesc1() }.unwrap();
            let len = d.Description.iter().position(|&c| c == 0).unwrap_or(0);
            eprintln!(
                "adapter {i}: {:?} vendor={:#06x} device={:#06x} subsys={:#010x} rev={} flags={} luid={:#x}:{:#x} dedicated={} MiB",
                String::from_utf16_lossy(&d.Description[..len]), d.VendorId, d.DeviceId, d.SubSysId, d.Revision, d.Flags,
                d.AdapterLuid.HighPart, d.AdapterLuid.LowPart, d.DedicatedVideoMemory / (1 << 20)
            );
            i += 1;
        }
    }
}
