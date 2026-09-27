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
}

impl SystemInfo {
    /// This Mac with `bytes` already taken by other loaded models, for
    /// planning a model that runs alongside them.
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
    if let Some(o) = override_bytes {
        return o.min(total_ram);
    }
    if total_ram >= 36 * GIB {
        total_ram / 4 * 3
    } else {
        total_ram / 3 * 2
    }
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
