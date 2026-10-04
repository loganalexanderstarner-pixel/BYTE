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
