//! Synthetic hardware fixtures: hardware we will never own, tested anyway.
//!
//! The planner (`system::plan_fit`, `chip::identify`) is pure arithmetic over a
//! description of a machine, which means every configuration BYTE will ever
//! meet can be tested without owning it. That matters because the Windows and
//! Linux targets are "any PC" -- thousands of CPU and GPU combinations, of
//! which we have exactly two to test on.
//!
//! The Apple fixtures below pass today and guard against regressions. The PC
//! fixtures are `#[ignore]`d: they assert what the planner MUST do once the
//! Windows work lands, and each one currently fails for a reason written into
//! the test. They are a specification, not a wish list -- remove the `ignore`
//! as each is implemented.

use crate::chip::{self, Backend, Tier};
use crate::system::{self, Fit, ModelArch};

const GIB: u64 = 1024 * 1024 * 1024;

/// A machine, as the planner sees one.
struct Machine {
    name: &'static str,
    cpu_brand: &'static str,
    total_ram: u64,
    /// Discrete GPU memory, separate from `total_ram`. None = unified (Apple)
    /// or no GPU at all.
    vram: Option<u64>,
}

const MACHINES: &[Machine] = &[
    // --- Apple: unified memory, GPU budget is a share of total RAM ----------
    Machine { name: "MacBook Air M4 8GB",   cpu_brand: "Apple M4",       total_ram: 8 * GIB,  vram: None },
    Machine { name: "MacBook Pro M4 Pro",   cpu_brand: "Apple M4 Pro",   total_ram: 24 * GIB, vram: None },
    Machine { name: "Mac Studio M4 Max",    cpu_brand: "Apple M4 Max",   total_ram: 64 * GIB, vram: None },
    // --- PC: discrete VRAM, a pool with no relationship to system RAM -------
    Machine { name: "RTX 5080 desktop",     cpu_brand: "AMD Ryzen 7 7800X3D",      total_ram: 32 * GIB, vram: Some(16 * GIB) },
    Machine { name: "GTX 1060 budget PC",   cpu_brand: "Intel Core i5-8400",       total_ram: 16 * GIB, vram: Some(6 * GIB) },
    Machine { name: "Arc A770 PC",          cpu_brand: "Intel Core Ultra 7 265K",  total_ram: 32 * GIB, vram: Some(16 * GIB) },
    Machine { name: "RX 7900 GRE PC",       cpu_brand: "AMD Ryzen 9 7900X",        total_ram: 32 * GIB, vram: Some(16 * GIB) },
    // --- PC: no usable GPU. The low-tier case the owner chose to support ----
    Machine { name: "office laptop",        cpu_brand: "Intel Core i5-1235U",      total_ram: 8 * GIB,  vram: None },
    Machine { name: "Snapdragon X Elite",   cpu_brand: "Snapdragon X Elite X1E-80-100", total_ram: 16 * GIB, vram: None },
];

/// Qwen3 4B shape, the smallest thing we would recommend on a weak machine.
fn small_model() -> ModelArch {
    ModelArch { n_layer: 36, kv_layers: 36, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
}

/// A 27B dense model: the thing a 16 GB card can hold and an 8 GB one cannot.
fn large_model() -> ModelArch {
    ModelArch { n_layer: 62, kv_layers: 62, n_head_kv: 16, head_dim: 128, max_ctx: 131072 }
}

// ---------------------------------------------------------------------------
// Passing today: the Apple path, guarded against regression.
// ---------------------------------------------------------------------------

#[test]
fn apple_chips_are_identified_with_real_numbers() {
    let m4 = chip::identify("Apple M4", Some(10));
    assert_eq!(m4.generation, 4, "M4 generation should parse from the brand");
    assert!(matches!(m4.tier, Tier::Base));
    assert!(m4.bandwidth_gbps > 100.0, "M4 bandwidth should be real, got {}", m4.bandwidth_gbps);

    let max = chip::identify("Apple M4 Max", Some(40));
    assert!(matches!(max.tier, Tier::Max));
    assert!(max.bandwidth_gbps > m4.bandwidth_gbps,
            "a Max should have more bandwidth than a base chip");
}

#[test]
fn a_4b_model_fits_every_machine_with_8gb_or_more() {
    // The owner's decision: low-tier hardware is supported, not excluded.
    let weights = 2_600_000_000u64; // 4B at Q4
    for m in MACHINES {
        let budget = system::gpu_budget(m.total_ram, None);
        let plan = system::plan_fit(weights, small_model(), 8192, m.total_ram, budget);
        assert!(!matches!(plan.fit, Fit::TooBig),
                "{}: a 4B model must run somewhere on every supported machine, got {:?} ({})",
                m.name, plan.fit, plan.note);
    }
}

#[test]
fn a_27b_model_does_not_fit_8gb() {
    let weights = 16_000_000_000u64;
    let total = 8 * GIB;
    let plan = system::plan_fit(weights, large_model(), 8192, total, system::gpu_budget(total, None));
    assert!(matches!(plan.fit, Fit::TooBig),
            "a 27B model must be refused on an 8 GB machine, not promised");
}

// ---------------------------------------------------------------------------
// The specification for Windows and Linux. Each fails today; the reason is in
// the message. Remove `ignore` as each is implemented.
// ---------------------------------------------------------------------------

#[test]
fn discrete_gpu_budget_never_exceeds_vram() {
    for m in MACHINES {
        let Some(vram) = m.vram else { continue };
        let budget = system::gpu_budget_for(m.total_ram, Some(vram), None);
        assert!(budget <= vram,
                "{}: planner offers {:.1} GB to a card holding {:.1} GB -- it would plan \
                 a model that cannot load",
                m.name, budget as f64 / GIB as f64, vram as f64 / GIB as f64);
    }
}

#[test]
fn intel_core_ultra_is_not_an_apple_ultra() {
    let c = chip::identify("Intel Core Ultra 7 265K", None);
    assert!(!matches!(c.tier, Tier::Ultra),
            "'Intel Core Ultra' must not be classified as an Apple Ultra tier");
    assert!(!c.exact, "an unknown PC chip must report exact=false, not fabricated numbers");
}

#[test]
fn amd_ryzen_pro_is_not_an_apple_pro() {
    let c = chip::identify("AMD Ryzen 7 PRO 7745", None);
    assert!(!matches!(c.tier, Tier::Pro),
            "'Ryzen PRO' must not be classified as an Apple Pro tier");
}

#[test]
#[ignore = "chip::identify has no PC data source: bandwidth, TFLOPS and core counts \
            all come from an Apple lookup table, so every PC gets zeroes or \
            extrapolations."]
fn pc_chips_report_usable_bandwidth() {
    for m in MACHINES {
        if m.cpu_brand.starts_with("Apple") { continue }
        let c = chip::identify(m.cpu_brand, None);
        assert!(c.bandwidth_gbps > 0.0,
                "{}: bandwidth must be known or estimated, not zero -- the planner \
                 predicts speed from it", m.name);
    }
}

#[test]
#[ignore = "ram_tier_gb returns Apple's shipping memory configurations. PCs have \
            arbitrary RAM, so the 'smallest machine that runs this' answer is \
            wrong off Apple hardware."]
fn ram_advice_is_not_limited_to_apple_configurations() {
    // A PC with 12 GB or 48 GB is ordinary; neither is an Apple tier.
    let needed = 10 * GIB;
    let tier = system::ram_tier_gb(needed);
    assert_ne!(tier, 16, "PC advice should not round to Apple's 16 GB tier by default");
}

#[test]
fn unified_memory_budget_is_unchanged() {
    // The Apple path must keep behaving exactly as before: a share of RAM.
    for gb in [8u64, 16, 24, 32, 64, 128] {
        let total = gb * GIB;
        assert_eq!(system::gpu_budget_for(total, None, None),
                   system::gpu_budget(total, None),
                   "{} GB unified: the Apple arithmetic must not change", gb);
    }
}

#[test]
fn a_user_override_cannot_exceed_the_physical_card() {
    // Someone raising the GPU share on a PC must still be bounded by VRAM.
    let vram = 8 * GIB;
    let b = system::gpu_budget_for(32 * GIB, Some(vram), Some(24 * GIB));
    assert!(b <= vram, "override offered {} bytes on an 8 GB card", b);
}

#[test]
fn small_cards_keep_a_usable_share() {
    // The reserve must not eat a low-end card alive: a 6 GB GPU should still
    // offer most of itself, or BYTE would refuse models that do fit.
    let b = system::gpu_budget_for(16 * GIB, Some(6 * GIB), None);
    assert!(b >= 5 * GIB, "6 GB card offered only {:.1} GB", b as f64 / GIB as f64);
}

// ---------------------------------------------------------------------------
// Measured on an RTX 5080, Qwen3 4B Q4_K_M, llama.cpp b11205:
//   CUDA    generation 204.8 tok/s   prompt 9,827 tok/s over 1,701 tokens
//   VULKAN  generation 192.1 tok/s   prompt   251 tok/s over 1,701 tokens
// The estimates must land near those, or every recommendation built on them
// misleads the user.
// ---------------------------------------------------------------------------

/// An RTX 5080: 960 GB/s, roughly 225 TFLOPS FP16 dense.
fn rtx_5080() -> chip::ChipInfo {
    let mut c = chip::identify("AMD Ryzen 7 7800X3D", None);
    c.bandwidth_gbps = 960.0;
    c.gpu_tflops = 225.0;
    c
}

const QWEN3_4B_Q4: u64 = 2_330_000_000;

#[test]
fn generation_estimate_is_near_the_measured_rate() {
    let c = rtx_5080();
    for (backend, measured) in [(Backend::Cuda, 204.8), (Backend::Vulkan, 192.1)] {
        let e = chip::estimate_on(&c, QWEN3_4B_Q4, Some(4.0), None, backend);
        let ratio = e.tokens_per_sec / measured;
        assert!((0.7..=1.4).contains(&ratio),
                "{backend:?}: estimated {:.0} tok/s against a measured {measured} \
                 (ratio {ratio:.2})", e.tokens_per_sec);
    }
}

#[test]
fn vulkan_prompt_speed_is_not_predicted_like_cuda() {
    // The whole point: the same card reads a document 39x slower on Vulkan, and
    // an estimate that misses that is wrong by an order of magnitude.
    let c = rtx_5080();
    let cuda = chip::estimate_on(&c, QWEN3_4B_Q4, Some(4.0), None, Backend::Cuda);
    let vk = chip::estimate_on(&c, QWEN3_4B_Q4, Some(4.0), None, Backend::Vulkan);
    assert!(cuda.prompt_per_sec > 10.0 * vk.prompt_per_sec,
            "cuda {:.0} vs vulkan {:.0} prompt tok/s -- the gap is measured at 39x",
            cuda.prompt_per_sec, vk.prompt_per_sec);
    // And generation should stay close, because it measured 1.07x.
    let g = cuda.tokens_per_sec / vk.tokens_per_sec;
    assert!((0.8..=1.3).contains(&g), "generation should be near-identical, got {g:.2}x");
}

#[test]
fn apple_estimates_are_untouched() {
    // estimate() must behave exactly as before for every existing caller.
    let m4 = chip::identify("Apple M4", Some(10));
    let old = chip::estimate(&m4, 4_900_000_000, Some(8.2), None);
    let new = chip::estimate_on(&m4, 4_900_000_000, Some(8.2), None, Backend::Metal);
    assert_eq!(old, new, "the Metal path must not have changed");
}

/// Measured on a Raspberry Pi 5 (4x Cortex-A76, 8 GB LPDDR4X ~17 GB/s, no GPU),
/// Qwen3 4B Q4_K_M, llama.cpp b11205, 3 threads: 3.26 tok/s generating,
/// 11.4 tok/s prompt. The weakest machine BYTE claims to support.
#[test]
fn low_tier_machines_get_an_honest_estimate_not_an_optimistic_one() {
    let mut pi = chip::identify("Cortex-A76", None);
    assert!(!pi.exact, "an ARM board is not in the Apple table and must say so");
    pi.bandwidth_gbps = 17.0; // what the hardware really has

    let e = chip::estimate_on(&pi, 2_400_000_000, Some(4.0), None, Backend::Cpu);
    // Must not promise more than roughly what it did: over-promising on the
    // weakest hardware is the worst case for trust.
    assert!(e.tokens_per_sec <= 6.0,
            "predicted {:.1} tok/s where the machine measured 3.26 -- \
             over-promising on low-tier hardware breaks the honest-limits rule",
            e.tokens_per_sec);
    assert!(e.tokens_per_sec >= 1.5,
            "predicted {:.1} tok/s, so pessimistic it would hide a usable machine",
            e.tokens_per_sec);
}

#[test]
fn the_unknown_hardware_default_is_not_optimistic() {
    // Before anything is measured, an unrecognised chip gets a default. It
    // should sit below a real desktop rather than above a small ARM board.
    let unknown = chip::identify("Some Future CPU 9000", None);
    assert!(!unknown.exact);
    assert!(unknown.bandwidth_gbps <= 30.0,
            "unknown hardware defaulted to {} GB/s, which over-promises",
            unknown.bandwidth_gbps);
}

// ---------------------------------------------------------------------------
// Phones (docs/ANDROID.md). llama.cpp runs on the CPU there, memory is shared
// like a Mac's, and Android -- not a GPU driver -- decides what an app may keep.
// Written before any Android device code, as the brief asks.
// ---------------------------------------------------------------------------

struct Phone {
    name: &'static str,
    /// What Android reports in `ro.soc.model`, or the marketing name.
    soc: &'static str,
    total_ram: u64,
}

const PHONES: &[Phone] = &[
    // The owner's phone. Exact parts unconfirmed until read over adb, so the
    // fixture is "a 16 GB foldable on the current Snapdragon 8 Elite class".
    Phone { name: "Galaxy Z Fold (16 GB, 8 Elite class)", soc: "SM8750", total_ram: 16 * GIB },
    Phone { name: "16 GB phone (Dimensity 9300)",         soc: "MT6989", total_ram: 16 * GIB },
    Phone { name: "6 GB mid-range phone",                 soc: "Snapdragon 7s Gen 2", total_ram: 6 * GIB },
];

/// Qwen3 1.7B shape: what a 6 GB phone should still run.
fn tiny_model() -> ModelArch {
    ModelArch { n_layer: 28, kv_layers: 28, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
}

/// Qwen3 8B / 14B shapes.
fn mid_model() -> ModelArch {
    ModelArch { n_layer: 36, kv_layers: 36, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
}
fn big_model() -> ModelArch {
    ModelArch { n_layer: 40, kv_layers: 40, n_head_kv: 8, head_dim: 128, max_ctx: 32768 }
}

fn phone(name: &str) -> &'static Phone {
    PHONES.iter().find(|p| p.name.starts_with(name)).expect("fixture")
}

#[test]
fn phone_chips_are_not_apple_and_say_they_are_estimates() {
    for p in PHONES {
        let c = chip::identify(p.soc, None);
        assert_eq!(c.generation, 0, "{}: a phone chip is not an M-series generation", p.name);
        assert!(!c.exact, "{}: phone numbers are peak figures until measured", p.name);
        assert!(c.bandwidth_gbps > 0.0 && c.bandwidth_gbps < 120.0,
                "{}: {} GB/s is not a phone's memory", p.name, c.bandwidth_gbps);
    }
    // A flagship has far more bandwidth than a mid-range phone; that difference is
    // most of the speed difference a user will see.
    let fold = chip::identify(phone("Galaxy").soc, None);
    let mid = chip::identify(phone("6 GB").soc, None);
    assert!(fold.bandwidth_gbps > 2.0 * mid.bandwidth_gbps);
    // The name Samsung uses in marketing finds the same row as the part number.
    assert_eq!(chip::identify("Snapdragon 8 Elite for Galaxy", None).bandwidth_gbps, fold.bandwidth_gbps);
}

#[test]
fn ai_focus_gives_the_model_most_of_a_16gb_phone() {
    let total = 16 * GIB;
    let focus = system::phone_budget(total, Some(5 * GIB), true, None);
    assert!((10 * GIB..=12 * GIB + GIB / 2).contains(&focus),
            "AI focus on 16 GB offered {:.1} GB", focus as f64 / GIB as f64);
    // Without AI focus, only what is free now, less a margin.
    let shy = system::phone_budget(total, Some(5 * GIB), false, None);
    assert!(shy < 5 * GIB && shy > 4 * GIB, "AI focus off offered {:.1} GB", shy as f64 / GIB as f64);
    // The memory test's measured ceiling wins over any arithmetic.
    assert_eq!(system::phone_budget(total, None, true, Some(9 * GIB)), 9 * GIB);
    // And Android always keeps enough to stay alive.
    for gb in [4u64, 6, 8, 12, 16, 24] {
        let t = gb * GIB;
        assert!(t - system::phone_budget(t, None, true, None) >= 5 * GIB / 2, "{gb} GB phone");
    }
}

#[test]
fn the_fold_runs_up_to_a_14b_with_ai_focus_and_refuses_a_27b() {
    let p = phone("Galaxy");
    let budget = system::phone_budget(p.total_ram, None, true, None);
    for (what, weights, arch) in [
        ("4B Q4", 2_400_000_000u64, small_model()),
        ("8B Q4", 5_000_000_000, mid_model()),
        ("14B Q4", 9_000_000_000, big_model()),
    ] {
        let plan = system::plan_fit_phone(weights, arch, 8192, p.total_ram, budget);
        assert!(!matches!(plan.fit, Fit::TooBig), "{what} should run on the Fold: {}", plan.note);
        assert!(plan.note.contains("phone") || plan.note.contains("context"), "{}", plan.note);
    }
    let plan = system::plan_fit_phone(16_000_000_000, large_model(), 8192, p.total_ram, budget);
    assert!(matches!(plan.fit, Fit::TooBig), "a 27B must be refused on a 16 GB phone");
    assert!(plan.note.contains("phone"), "{}", plan.note);
}

#[test]
fn a_6gb_phone_runs_a_small_model_and_refuses_an_8b() {
    let p = phone("6 GB");
    let budget = system::phone_budget(p.total_ram, None, true, None);
    let ok = system::plan_fit_phone(1_100_000_000, tiny_model(), 8192, p.total_ram, budget);
    assert!(!matches!(ok.fit, Fit::TooBig), "1.7B Q4 on 6 GB: {}", ok.note);
    let no = system::plan_fit_phone(5_000_000_000, mid_model(), 8192, p.total_ram, budget);
    assert!(matches!(no.fit, Fit::TooBig), "an 8B must not be promised on a 6 GB phone");
}

#[test]
fn phone_speed_estimates_are_honest() {
    // Calibrated the only way available before the Fold is connected: the Pi 5
    // measurement (memory bandwidth x the CPU's achievable share) scaled by each
    // phone's memory. The Fold's real number replaces this on first measure.
    let fold = chip::identify(phone("Galaxy").soc, None);
    let mid = chip::identify(phone("6 GB").soc, None);
    let e4 = chip::estimate_on(&fold, 2_400_000_000, Some(4.0), None, Backend::Cpu);
    assert!((8.0..=25.0).contains(&e4.tokens_per_sec),
            "4B Q4 on a flagship phone: {:.1} tok/s", e4.tokens_per_sec);
    let m17 = chip::estimate_on(&mid, 1_100_000_000, Some(1.7), None, Backend::Cpu);
    let m4 = chip::estimate_on(&mid, 2_400_000_000, Some(4.0), None, Backend::Cpu);
    assert!(m4.tokens_per_sec < e4.tokens_per_sec && m17.tokens_per_sec > m4.tokens_per_sec);
    // Reading a prompt is not five minutes: the CPU floor ties it to generation.
    assert!(e4.prompt_per_sec >= 3.0 * e4.tokens_per_sec, "{e4:?}");
    assert!(e4.reply_secs < 120.0, "a typical answer on the Fold: {:.0} s", e4.reply_secs);
}

#[test]
fn cpu_prompt_estimate_matches_the_pi_measurement() {
    let mut pi = chip::identify("Cortex-A76", None);
    pi.bandwidth_gbps = 17.0;
    let e = chip::estimate_on(&pi, 2_400_000_000, Some(4.0), None, Backend::Cpu);
    let ratio = e.prompt_per_sec / 11.4;
    assert!((0.7..=1.4).contains(&ratio), "predicted {:.1} prompt tok/s, measured 11.4", e.prompt_per_sec);
}
