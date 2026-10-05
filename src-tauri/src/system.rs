//! Hardware detection and the RAM planner that decides whether a model can run
//! on this Mac and how much context it can afford.

use serde::Serialize;
use sysinfo::{Disks, System};

const GB: u64 = 1_000_000_000;
const GIB: u64 = 1 << 30;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub chip: String,
    pub total_ram_bytes: u64,
    pub gpu_budget_bytes: u64,
    pub free_disk_bytes: u64,
    pub os_version: String,
    pub cpu_cores: usize,
    pub apple_silicon: bool,
    /// Chip generation, tier, bandwidth and Neural Engine (for speed estimates).
    pub chip_info: crate::chip::ChipInfo,
    /// What the user asked BYTE to favour when recommending (set from settings).
    #[serde(skip)]
    pub speed_pref: crate::settings::SpeedPref,
    /// Speed boost is on (estimates count on it where a helper exists).
    #[serde(skip)]
    pub boost: bool,
    /// Writing speed measured by tuning on this Mac, by model key.
    #[serde(skip)]
    pub measured: std::collections::HashMap<String, f64>,
    /// Measured ÷ estimated speed on this Mac (median over tuned models), to
    /// correct estimates for models not measured yet. Set by `models::calibrate`.
    #[serde(skip)]
    pub calibration: Option<f64>,
}

impl SystemInfo {
    /// This Mac with `bytes` already taken by other loaded models, for
    /// planning a model that runs alongside them.
    #[cfg(test)]
    pub fn with_pref(mut self, pref: crate::settings::SpeedPref) -> Self {
        self.speed_pref = pref;
        self
    }

    /// Adds what the settings say about preferences and measured speeds.
    pub fn with_settings(mut self, s: &crate::settings::Settings) -> Self {
        let chip = self.chip_id();
        self.speed_pref = s.speed_pref;
        self.boost = s.speed_boost;
        self.measured = s.tuning.iter().filter(|(_, t)| t.chip == chip && t.tokens_per_sec > 0.0).map(|(k, t)| (k.clone(), t.tokens_per_sec)).collect();
        self
    }

    /// Name of this Mac's chip, to know when tuning was done on another Mac.
    pub fn chip_id(&self) -> String {
        let c = &self.chip_info;
        format!("{} {}", c.name, c.gpu_cores.map(|g| format!("{g}-core GPU")).unwrap_or_default()).trim().to_string()
    }

    pub fn minus(mut self, bytes: u64) -> Self {
        self.gpu_budget_bytes = self.gpu_budget_bytes.saturating_sub(bytes);
        self.total_ram_bytes = self.total_ram_bytes.saturating_sub(bytes);
        self
    }
}

pub fn system_info(data_dir: &std::path::Path) -> SystemInfo {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let total = sys.total_memory();
    let chip = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "Unknown".into());
    static GPU_CORES: once_cell::sync::Lazy<Option<u32>> = once_cell::sync::Lazy::new(crate::chip::gpu_core_count);
    SystemInfo {
        chip_info: crate::chip::identify(&chip, *GPU_CORES),
        speed_pref: Default::default(),
        boost: false,
        measured: Default::default(),
        calibration: None,
        apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        gpu_budget_bytes: gpu_budget(total, wired_limit_override()),
        free_disk_bytes: free_disk_for(data_dir),
        os_version: System::long_os_version().unwrap_or_default(),
        cpu_cores: sys.cpus().len(),
        total_ram_bytes: total,
        chip,
    }
}

/// Approximates Metal's `recommendedMaxWorkingSetSize`: macOS lets the GPU wire
/// about two thirds of RAM on smaller Macs and three quarters on 36 GB+.
/// If the user raised `iogpu.wired_limit_mb`, that wins.
pub fn gpu_budget(total_ram: u64, override_bytes: Option<u64>) -> u64 {
    gpu_budget_for(total_ram, None, override_bytes)
}

/// How much GPU memory a model may use.
///
/// Two different machines hide behind this one number:
///
/// * **Unified memory** (Apple silicon): the GPU draws from system RAM, so a
///   share of total RAM is exactly right, and that is what `vram: None` means.
/// * **A discrete card** (every PC with a real GPU): VRAM is a separate, fixed
///   pool with no relationship to system RAM. Deriving the budget from RAM
///   over-commits the card badly -- on a 32 GB PC the old arithmetic offered
///   21.3 GB to a 16 GB RTX 5080, so BYTE would recommend a model, the user
///   would pick it, and the load would fail. Found by the fixtures in
///   `hardware_fixtures_tests.rs`, which is the whole reason they exist.
///
/// The reserve exists because on a PC the same card is drawing the desktop and
/// whatever else is open. It is deliberately generous rather than optimistic:
/// promising a model that then fails to load is worse than recommending a
/// slightly smaller one.
pub fn gpu_budget_for(total_ram: u64, vram: Option<u64>, override_bytes: Option<u64>) -> u64 {
    if let Some(v) = vram {
        // A user override still cannot exceed the physical card.
        if let Some(o) = override_bytes {
            return o.min(v);
        }
        let reserve = (v / 10).clamp(512 * 1024 * 1024, 3 * GIB / 2);
        return v.saturating_sub(reserve);
    }
    if let Some(o) = override_bytes {
        return o.min(total_ram);
    }
    if total_ram >= 36 * GIB {
        total_ram / 4 * 3
    } else {
        total_ram / 3 * 2
    }
}

/// A larger GPU share that still leaves macOS enough memory: all but 4 GB,
/// or all but 12.5% on big Macs (16 GB → 12 GB, 32 GB → 28 GB, 64 GB → 56 GB).
pub fn raised_gpu_budget(total_ram: u64) -> u64 {
    total_ram.saturating_sub((total_ram / 8).max(4 * GIB)).max(gpu_budget(total_ram, None))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuShare {
    /// Whether this Mac supports changing it (Apple Silicon macOS).
    pub supported: bool,
    pub current_bytes: u64,
    pub default_bytes: u64,
    pub raised_bytes: u64,
    pub raised: bool,
}

pub fn gpu_share() -> GpuShare {
    let mut sys = System::new();
    sys.refresh_memory();
    let total = sys.total_memory();
    let over = wired_limit_override();
    GpuShare {
        supported: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        current_bytes: gpu_budget(total, over),
        default_bytes: gpu_budget(total, None),
        raised_bytes: raised_gpu_budget(total),
        raised: over.is_some(),
    }
}

/// Raises (or resets) how much memory macOS lets the GPU use. Asks for the
/// administrator password through the standard macOS dialog; lasts until the
/// Mac restarts.
pub fn set_gpu_share(raise: bool) -> crate::error::AppResult<GpuShare> {
    #[cfg(target_os = "macos")]
    {
        let mb = if raise { raised_gpu_budget(gpu_share_total()) / (1024 * 1024) } else { 0 };
        let script = format!("do shell script \"/usr/sbin/sysctl iogpu.wired_limit_mb={mb}\" with administrator privileges");
        let out = std::process::Command::new("/usr/bin/osascript").args(["-e", &script]).output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(crate::error::AppError::msg(if err.contains("-128") {
                "Cancelled.".to_string()
            } else {
                format!("macOS didn't allow the change: {}", err.trim())
            }));
        }
        Ok(gpu_share())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = raise;
        Err(crate::error::AppError::msg("This setting is only available on Macs."))
    }
}

#[cfg(target_os = "macos")]
fn gpu_share_total() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.total_memory()
}

fn wired_limit_override() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "iogpu.wired_limit_mb"])
            .output()
            .ok()?;
        let mb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        if mb > 0 {
            return Some(mb * 1024 * 1024);
        }
    }
    None
}

pub fn free_disk_for(path: &std::path::Path) -> u64 {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0)
}

/// Shape of a transformer that matters for KV-cache size (read from the GGUF
/// header by scripts/build-catalog.mjs).
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelArch {
    pub n_layer: u32,
    /// Layers that keep a KV cache. Hybrid models (Qwen3.5+, LFM2) only cache
    /// their full-attention layers, which makes long contexts much cheaper.
    pub kv_layers: u32,
    pub n_head_kv: u32,
    pub head_dim: u32,
    pub max_ctx: u32,
}

impl ModelArch {
    /// Bytes of KV cache per token with q8_0 K and V (34 bytes per 32 values).
    pub fn kv_bytes_per_token(&self) -> u64 {
        let layers = if self.kv_layers > 0 { self.kv_layers } else { self.n_layer };
        let values = 2 * layers as u64 * self.n_head_kv as u64 * self.head_dim as u64;
        values * 34 / 32
    }
}

/// Smallest standard Mac memory size (GB) that can run a model needing
/// `needed_bytes` of GPU memory, given macOS's GPU share of RAM.
pub fn ram_tier_gb(needed_bytes: u64) -> u32 {
    const TIERS: [u32; 11] = [8, 16, 24, 32, 36, 48, 64, 96, 128, 192, 256];
    for t in TIERS {
        let total = t as u64 * GIB;
        if gpu_budget(total, None) >= needed_bytes && total >= needed_bytes + OS_RESERVE {
            return t;
        }
    }
    512
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Runs comfortably with room for other apps.
    Great,
    /// Runs, but close other heavy apps.
    Tight,
    /// Will not run on this Mac.
    TooBig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FitPlan {
    pub fit: Fit,
    pub context: u32,
    pub needed_bytes: u64,
    pub gpu_budget_bytes: u64,
    pub total_ram_bytes: u64,
    pub note: String,
    /// Mixture-of-experts layers whose experts stay in regular memory for the
    /// CPU (llama.cpp `--n-cpu-moe`), when the model is larger than the GPU's
    /// share of memory but fits in RAM. 0 = everything on the GPU.
    pub cpu_moe_layers: u32,
    /// Layers on the GPU when a dense model is slightly too big ("stretch";
    /// the rest run on the CPU, noticeably slower). None = all.
    pub gpu_layers: Option<u32>,
}

impl FitPlan {
    /// Part of the model runs on the CPU.
    pub fn offloaded(&self) -> bool {
        self.cpu_moe_layers > 0 || self.gpu_layers.is_some()
    }
}

/// Compute buffers, Metal heaps and scratch space on top of weights + KV.
const RUNTIME_OVERHEAD: u64 = 700 * 1_000_000;
const MIN_CONTEXT: u32 = 4096;
/// What macOS and a browser need to stay responsive.
const OS_RESERVE: u64 = 3 * GB;

/// Memory to leave for other apps before calling a fit "comfortable":
/// 30% of RAM, between 3 and 5 GB.
fn comfort_reserve(total_ram: u64) -> u64 {
    (total_ram * 3 / 10).clamp(3 * GB, 5 * GB)
}

pub fn plan_fit(
    weights_bytes: u64,
    arch: ModelArch,
    desired_ctx: u32,
    total_ram: u64,
    gpu_budget: u64,
) -> FitPlan {
    let per_tok = arch.kv_bytes_per_token().max(1);
    let desired = desired_ctx.min(arch.max_ctx).max(MIN_CONTEXT);
    let base = weights_bytes + RUNTIME_OVERHEAD;
    let too_big = |note: String| FitPlan {
        fit: Fit::TooBig,
        context: 0,
        needed_bytes: base + per_tok * MIN_CONTEXT as u64,
        gpu_budget_bytes: gpu_budget,
        total_ram_bytes: total_ram,
        note,
        cpu_moe_layers: 0,
        gpu_layers: None,
    };

    let gpu_room = gpu_budget.saturating_sub(base);
    let ram_room = total_ram.saturating_sub(base + OS_RESERVE);
    let room = gpu_room.min(ram_room);
    if room < per_tok * MIN_CONTEXT as u64 {
        let need_gb = (base + per_tok * MIN_CONTEXT as u64) as f64 / GB as f64;
        let ram_needed = ((need_gb * 1.5 / 8.0).ceil() * 8.0) as u64;
        return too_big(format!(
            "Needs about {need_gb:.1} GB of GPU memory. A Mac with {}+ GB of RAM is required.",
            ram_needed.max(16)
        ));
    }

    let affordable = (room / per_tok).min(u32::MAX as u64) as u32;
    // Round down to a multiple of 1024 tokens.
    let context = (affordable.min(desired) / 1024 * 1024).max(MIN_CONTEXT);
    let needed = base + per_tok * context as u64;
    let left = total_ram.saturating_sub(needed);
    let (fit, note) = if left >= comfort_reserve(total_ram) && context >= desired {
        (Fit::Great, "Runs comfortably on this Mac.".to_string())
    } else if context < desired {
        (
            Fit::Tight,
            format!("Runs with a reduced context of {}k tokens to fit in memory.", context / 1024),
        )
    } else {
        (Fit::Tight, "Runs, but close other heavy apps for best speed.".to_string())
    };
    FitPlan {
        fit,
        context,
        needed_bytes: needed,
        gpu_budget_bytes: gpu_budget,
        total_ram_bytes: total_ram,
        note,
        cpu_moe_layers: 0,
        gpu_layers: None,
    }
}

/// How much memory a model may use on a phone (Android, `docs/ANDROID.md`).
///
/// A phone has unified memory like a Mac, but Android, not a GPU driver, decides
/// what an app may keep: take too much and the low-memory killer ends BYTE
/// mid-answer. Two cases:
///
/// * **AI focus on** (BYTE on screen or Ask BYTE in use): background apps can be
///   moved out to RAM Plus / swap, so the budget is all of RAM except what
///   Android itself needs -- a quarter of RAM, between 2.5 and 4.5 GB.
/// * **AI focus off**: only what's free right now (`MemAvailable`), less a
///   margin, so other apps stay in RAM.
///
/// `measured` is the ceiling found by the per-phone memory test, when there is
/// one; it always wins, because it is what this phone actually allowed.
pub fn phone_budget(total_ram: u64, available: Option<u64>, ai_focus: bool, measured: Option<u64>) -> u64 {
    if let Some(m) = measured {
        return m.min(total_ram);
    }
    let android = (total_ram / 4).clamp(5 * GIB / 2, 9 * GIB / 2);
    let focus = total_ram.saturating_sub(android);
    if ai_focus {
        return focus;
    }
    match available {
        Some(a) => a.saturating_sub(GIB / 2).min(focus),
        None => focus / 2,
    }
}

/// `plan_fit` for a phone: the budget already accounts for Android, so no
/// desktop OS reserve is taken again, and the notes say "phone".
pub fn plan_fit_phone(weights_bytes: u64, arch: ModelArch, desired_ctx: u32, total_ram: u64, budget: u64) -> FitPlan {
    let mut plan = plan_fit(weights_bytes, arch, desired_ctx, budget + OS_RESERVE, budget);
    plan.total_ram_bytes = total_ram;
    plan.note = if plan.fit == Fit::TooBig {
        let need = plan.needed_bytes as f64 / GB as f64;
        format!("Needs about {need:.1} GB free for the model; this phone can give it {:.1} GB.", budget as f64 / GB as f64)
    } else {
        plan.note.replace("this Mac", "this phone")
    };
    plan
}

/// Dense models may run at most this share of their weights on the CPU.
const MAX_STRETCH: f64 = 0.15;

/// With part of a model on the CPU, the whole file still sits in memory, so
/// macOS, BYTE's window and the GPU driver need this much left over (3 GB
/// wasn't enough on 16 GB Macs: big MoE models got killed while loading).
const OFFLOAD_OS_RESERVE: u64 = 4 * GB;
/// GPU memory kept free for llama.cpp's working buffers when offloading
/// (0.3 GB made Metal fail to allocate them for 35B MoE models).
const OFFLOAD_GPU_MARGIN: u64 = 1_000_000_000;

/// For a model that doesn't fit the GPU's share of memory: runs part of it on
/// the CPU when the whole model still fits in RAM (Apple Silicon memory is
/// shared, so nothing is copied). Mixture-of-experts models move whole expert
/// layers, which costs little speed because each token uses few experts;
/// dense models move a few layers ("stretch"), which costs more. Returns
/// `None` when that doesn't help either.
///
/// `expert_share`: fraction of the weights that are experts (0 for dense).
pub fn plan_offload(weights_bytes: u64, arch: ModelArch, desired_ctx: u32, total_ram: u64, gpu_budget: u64, expert_share: f64) -> Option<FitPlan> {
    let per_tok = arch.kv_bytes_per_token().max(1);
    let desired = desired_ctx.min(arch.max_ctx).max(MIN_CONTEXT);
    let base = weights_bytes + RUNTIME_OVERHEAD;
    let ram_room = total_ram.checked_sub(base + OFFLOAD_OS_RESERVE)?;
    if ram_room < per_tok * MIN_CONTEXT as u64 || arch.n_layer == 0 {
        return None;
    }
    let context = ((ram_room / per_tok).min(desired as u64) as u32 / 1024 * 1024).max(MIN_CONTEXT);
    let needed = base + per_tok * context as u64;
    // What must leave the GPU, with a little margin for buffers.
    let excess = (needed + OFFLOAD_GPU_MARGIN).saturating_sub(gpu_budget);
    if excess == 0 {
        return None; // fits on the GPU: the normal plan applies
    }
    let layers = arch.n_layer as u64;
    let (cpu_moe_layers, gpu_layers, note) = if expert_share > 0.3 {
        let per_layer = (weights_bytes as f64 * expert_share / layers as f64).max(1.0);
        let n = ((excess as f64 / per_layer).ceil() as u64 + 1).min(layers);
        if n as f64 * per_layer < excess as f64 {
            return None;
        }
        (n as u32, None, format!("Runs with {n} of {layers} expert layers on the CPU, a little slower. Close other heavy apps."))
    } else {
        if excess as f64 > weights_bytes as f64 * MAX_STRETCH {
            return None;
        }
        let per_layer = (weights_bytes / layers).max(1);
        let off = excess.div_ceil(per_layer) + 1;
        let on = layers.saturating_sub(off);
        (0, Some(on as u32), format!("Stretch mode: {off} of {layers} layers run on the CPU, noticeably slower. Close other heavy apps."))
    };
    Some(FitPlan {
        fit: Fit::Tight,
        context,
        needed_bytes: needed,
        gpu_budget_bytes: gpu_budget,
        total_ram_bytes: total_ram,
        note,
        cpu_moe_layers,
        gpu_layers,
    })
}

#[cfg(test)]
mod gpu_share_tests {
    use super::*;

    #[test]
    fn raised_share_leaves_room_for_macos() {
        assert_eq!(raised_gpu_budget(16 * GIB), 12 * GIB);
        assert_eq!(raised_gpu_budget(32 * GIB), 28 * GIB);
        assert_eq!(raised_gpu_budget(64 * GIB), 56 * GIB);
        for gb in [8u64, 16, 24, 36, 128] {
            assert!(raised_gpu_budget(gb * GIB) >= gpu_budget(gb * GIB, None), "{gb}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAM16: u64 = 16 * GIB;
    const RAM24: u64 = 24 * GIB;

    fn qwen14() -> ModelArch {
        ModelArch { n_layer: 40, kv_layers: 40, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
    }
    fn qwen8() -> ModelArch {
        ModelArch { n_layer: 36, kv_layers: 36, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
    }
    fn qwen30a3() -> ModelArch {
        ModelArch { n_layer: 48, kv_layers: 48, n_head_kv: 4, head_dim: 128, max_ctx: 32768 }
    }

    #[test]
    fn kv_per_token_q8() {
        // 2 * 40 * 8 * 128 = 81920 values -> 87040 bytes at q8_0.
        assert_eq!(qwen14().kv_bytes_per_token(), 87_040);
    }

    #[test]
    fn default_14b_fits_16gb_with_16k_context() {
        let p = plan_fit(9_001_752_960, qwen14(), 16384, RAM16, gpu_budget(RAM16, None));
        assert_ne!(p.fit, Fit::TooBig, "{p:?}");
        assert_eq!(p.context, 16384);
    }

    #[test]
    fn eight_b_is_great_on_16gb() {
        let p = plan_fit(5_027_783_488, qwen8(), 16384, RAM16, gpu_budget(RAM16, None));
        assert_eq!(p.fit, Fit::Great);
    }

    #[test]
    fn thirty_b_q3_does_not_fit_16gb_but_fits_24gb() {
        let p = plan_fit(14_711_847_488, qwen30a3(), 16384, RAM16, gpu_budget(RAM16, None));
        assert_eq!(p.fit, Fit::TooBig);
        assert!(p.note.contains("GB"));
        let p24 = plan_fit(14_711_847_488, qwen30a3(), 16384, RAM24, gpu_budget(RAM24, None));
        assert_ne!(p24.fit, Fit::TooBig, "{p24:?}");
    }

    #[test]
    fn large_context_request_is_reduced_not_rejected() {
        let p = plan_fit(9_001_752_960, qwen14(), 32768, RAM16, gpu_budget(RAM16, None));
        assert_eq!(p.fit, Fit::Tight);
        assert!(p.context < 32768 && p.context >= 16384, "{p:?}");
        assert_eq!(p.context % 1024, 0);
    }

    #[test]
    fn hybrid_models_only_count_attention_layers() {
        let hybrid = ModelArch { n_layer: 64, kv_layers: 16, n_head_kv: 4, head_dim: 256, max_ctx: 262144 };
        let full = ModelArch { kv_layers: 64, ..hybrid };
        assert_eq!(full.kv_bytes_per_token(), hybrid.kv_bytes_per_token() * 4);
    }

    #[test]
    fn ram_tiers() {
        assert_eq!(ram_tier_gb(3 * GB), 8);
        assert_eq!(ram_tier_gb(10 * GB), 16);
        assert_eq!(ram_tier_gb(15 * GB), 24);
        assert_eq!(ram_tier_gb(64 * GB), 96);
    }

    #[test]
    fn budget_override_is_respected() {
        assert_eq!(gpu_budget(RAM16, Some(12 * GIB)), 12 * GIB);
        assert_eq!(gpu_budget(RAM16, Some(64 * GIB)), RAM16);
        assert_eq!(gpu_budget(48 * GIB, None), 36 * GIB);
    }
}

/// Battery charge (percent) and whether it's charging or plugged in; None on desktops.
pub fn battery() -> Option<(u8, bool)> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("pmset").args(["-g", "batt"]).output().ok()?;
        parse_pmset(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(target_os = "linux")]
    {
        let dir = std::path::Path::new("/sys/class/power_supply/BAT0");
        let pct: u8 = std::fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()?;
        let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
        Some((pct, !status.trim().eq_ignore_ascii_case("discharging")))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// `pmset -g batt`: "Now drawing from 'Battery Power' … 18%; discharging; 1:52 remaining".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_pmset(text: &str) -> Option<(u8, bool)> {
    let line = text.lines().find(|l| l.contains('%'))?;
    let pct: u8 = line.split('%').next()?.rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let on_ac = text.contains("'AC Power'");
    let charging = on_ac || (line.contains("charging") && !line.contains("discharging")) || line.contains("charged");
    Some((pct.min(100), charging))
}

/// Battery saver applies: under 20% and not plugged in.
pub fn low_battery() -> bool {
    battery().is_some_and(|(p, charging)| p < 20 && !charging)
}

#[cfg(test)]
mod battery_tests {
    use super::parse_pmset;

    #[test]
    fn pmset_output_is_read() {
        let on_battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t18%; discharging; 1:52 remaining present: true\n";
        assert_eq!(parse_pmset(on_battery), Some((18, false)));
        let charging = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t57%; charging; 0:48 remaining present: true\n";
        assert_eq!(parse_pmset(charging), Some((57, true)));
        let full = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true";
        assert_eq!(parse_pmset(full), Some((100, true)));
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), None, "a desktop Mac has no battery");
    }
}

#[cfg(test)]
#[path = "hardware_fixtures_tests.rs"]
mod hardware_fixtures_tests;
