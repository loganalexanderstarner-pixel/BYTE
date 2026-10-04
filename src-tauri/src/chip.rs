//! Identifies the Apple Silicon chip and estimates how fast models run on it.
//!
//! Token generation on Apple Silicon is limited by memory bandwidth (every
//! generated token reads the model's active weights once), while reading the
//! prompt is limited by GPU compute. Both are known per chip, so BYTE can
//! estimate speed for any model before it's downloaded — and an M4 is
//! correctly shown as faster than an M2.
//!
//! The Neural Engine isn't used by llama.cpp for chat (it runs on the GPU);
//! BYTE uses it through Apple's frameworks for OCR and speech (Phases 4, 11).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Base,
    Pro,
    Max,
    Ultra,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChipInfo {
    /// Marketing name, e.g. "Apple M4 Pro".
    pub name: String,
    /// 1 for M1, 4 for M4, … (0 when unknown, e.g. Intel).
    pub generation: u8,
    pub tier: Tier,
    pub gpu_cores: Option<u32>,
    /// Unified memory bandwidth, GB/s.
    pub bandwidth_gbps: f64,
    /// Approximate GPU FP16 throughput, TFLOPS.
    pub gpu_tflops: f64,
    /// Neural Engine throughput, trillions of operations per second.
    pub neural_engine_tops: f64,
    /// False when the numbers are extrapolated for an unlisted chip.
    pub exact: bool,
}

/// Parses a CPU brand like "Apple M4 Pro" and the GPU core count.
pub fn identify(brand: &str, gpu_cores: Option<u32>) -> ChipInfo {
    let b = brand.to_lowercase();
    // Apple-only parsing, gated on the brand actually being an Apple chip.
    //
    // Without that gate the tier keywords match any CPU name containing them,
    // and PC brands are full of them: "Intel Core Ultra 7 265K" came out as an
    // Apple ULTRA tier and "AMD Ryzen 7 PRO" as an Apple PRO, each then handed
    // an M-series chip's bandwidth and TFLOPS out of the table below -- numbers
    // the planner predicts speed from and the catalogue recommends models from.
    // Found by the fixtures in hardware_fixtures_tests.rs.
    let apple = b.contains("apple")
        || b.split_whitespace().any(|w| {
            w.len() >= 2
                && w.starts_with('m')
                && w[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
        });
    let generation = if apple {
        b.split_whitespace()
            .find_map(|w| w.strip_prefix('m').and_then(|n| n.parse::<u8>().ok()))
            .unwrap_or(0)
    } else {
        0
    };
    let tier = if !apple {
        // A PC chip has no Apple tier. Saying Base is not a guess about the
        // hardware; it keeps it out of the Apple lookup table entirely, and
        // `exact: false` below is what tells callers the numbers are estimates.
        Tier::Base
    } else if b.contains("ultra") {
        Tier::Ultra
    } else if b.contains("max") {
        Tier::Max
    } else if b.contains("pro") {
        Tier::Pro
    } else {
        Tier::Base
    };
    let cores = gpu_cores.unwrap_or(0);
    // (bandwidth GB/s, GPU TFLOPS FP16, Neural Engine TOPS)
    let known = match (generation, tier) {
        (1, Tier::Base) => Some((68.25, 2.6, 11.0)),
        (1, Tier::Pro) => Some((200.0, 5.2, 11.0)),
        (1, Tier::Max) => Some((400.0, 10.4, 11.0)),
        (1, Tier::Ultra) => Some((800.0, 21.0, 22.0)),
        (2, Tier::Base) => Some((100.0, 3.6, 15.8)),
        (2, Tier::Pro) => Some((200.0, 6.8, 15.8)),
        (2, Tier::Max) => Some((400.0, 13.6, 15.8)),
        (2, Tier::Ultra) => Some((800.0, 27.2, 31.6)),
        (3, Tier::Base) => Some((100.0, 4.1, 18.0)),
        (3, Tier::Pro) => Some((150.0, 7.4, 18.0)),
        // The 30-core M3 Max has 300 GB/s, the 40-core 400 GB/s.
        (3, Tier::Max) => Some(if cores > 0 && cores <= 30 { (300.0, 10.6, 18.0) } else { (400.0, 14.2, 18.0) }),
        (3, Tier::Ultra) => Some((819.0, 28.4, 36.0)),
        (4, Tier::Base) => Some((120.0, 4.3, 38.0)),
        (4, Tier::Pro) => Some((273.0, 9.2, 38.0)),
        // The 32-core M4 Max has 410 GB/s, the 40-core 546 GB/s.
        (4, Tier::Max) => Some(if cores > 0 && cores <= 32 { (410.0, 14.7, 38.0) } else { (546.0, 18.4, 38.0) }),
        (5, Tier::Base) => Some((153.0, 5.7, 38.0)),
        _ => None,
    };
    let (bandwidth_gbps, gpu_tflops, neural_engine_tops, exact) = match known {
        Some((bw, tf, ne)) => (bw, tf, ne, true),
        None if apple && generation >= 4 => {
            // Newer or unlisted chip: scale the newest known base chip by tier.
            let mult = match tier {
                Tier::Base => 1.0,
                Tier::Pro => 2.0,
                Tier::Max => 3.6,
                Tier::Ultra => 7.2,
            };
            (153.0 * mult, 5.7 * mult, 38.0, false)
        }
        None => (50.0, 1.0, 0.0, false),
    };
    ChipInfo {
        name: if brand.trim().is_empty() { "Unknown".into() } else { brand.trim().to_string() },
        generation,
        tier,
        gpu_cores,
        bandwidth_gbps,
        gpu_tflops,
        neural_engine_tops,
        exact,
    }
}

/// Reads the GPU core count from IOKit (macOS only).
pub fn gpu_core_count() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/ioreg").args(["-rc", "AGXAccelerator", "-d", "1"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if let Some(rest) = line.split("\"gpu-core-count\" = ").nth(1) {
                return rest.trim().parse().ok();
            }
        }
    }
    None
}

/// Which engine will actually serve the model. It belongs in a speed estimate
/// because the same GPU is a different machine depending on the answer:
/// measured on an RTX 5080 with Qwen3 4B Q4_K_M, generation came out 204.8
/// tok/s on CUDA and 192.1 on Vulkan -- near enough identical -- while prompt
/// processing was 9,827 tok/s against 251, a factor of **39**. Predicting from
/// hardware alone is therefore right about generation and wrong by more than an
/// order of magnitude about reading a document.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// Apple unified memory. The existing numbers were tuned here.
    Metal,
    /// NVIDIA through CUDA: cuBLAS makes prompt processing enormously faster.
    Cuda,
    /// Any vendor through Vulkan. Generation matches CUDA; prompt does not.
    Vulkan,
    /// No usable GPU.
    Cpu,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeedEstimate {
    /// Generated tokens per second.
    pub tokens_per_sec: f64,
    /// Prompt tokens read per second.
    pub prompt_per_sec: f64,
    /// Seconds for a typical answer (~350 words out, a short conversation in).
    pub reply_secs: f64,
    /// Same, with the model thinking first.
    pub reply_thinking_secs: f64,
}

/// Typical sizes used for the reply-time estimate.
const TYPICAL_PROMPT: f64 = 1500.0;
const TYPICAL_ANSWER: f64 = 450.0;
const TYPICAL_THINKING: f64 = 700.0;

/// Estimates speed for a model file of `file_bytes` with `total_b` billion
/// parameters of which `active_b` are used per token (MoE), on `chip`.
pub fn estimate(chip: &ChipInfo, file_bytes: u64, total_b: Option<f32>, active_b: Option<f32>) -> SpeedEstimate {
    estimate_on(chip, file_bytes, total_b, active_b, Backend::Metal)
}

/// As `estimate`, told which engine will serve the model.
///
/// The two correction factors below are measured, not assumed, and that matters
/// because the uncorrected arithmetic is badly wrong off Apple hardware. For
/// Qwen3 4B Q4_K_M (2.33 GB) on an RTX 5080 (960 GB/s) the bandwidth model
/// predicts 329 tok/s at the Apple efficiency of 0.8; the card actually
/// delivered 204.8 on CUDA and 192.1 on Vulkan, so the achievable share of peak
/// on a discrete card is nearer 0.6 than 0.8 -- it overpredicted by 45%.
///
/// One GPU, one model, one build: a single calibration point, honestly, and the
/// right fix long term is `models::calibrate` measuring the real machine. These
/// factors exist so the first estimate a user ever sees is not fiction.
pub fn estimate_on(
    chip: &ChipInfo,
    file_bytes: u64,
    total_b: Option<f32>,
    active_b: Option<f32>,
    backend: Backend,
) -> SpeedEstimate {
    let total = total_b.map(|t| t as f64).filter(|t| *t > 0.0).unwrap_or(file_bytes as f64 / 0.6e9);
    let active = active_b.map(|a| a as f64).filter(|a| *a > 0.0 && *a < total).unwrap_or(total);
    let moe = active < total;
    // Bytes of weights touched per generated token.
    let bytes_per_token = file_bytes as f64 * (active / total);
    // Achievable share of peak bandwidth (MoE routing is less efficient).
    let mut efficiency = if moe { 0.6 } else { 0.8 };
    // Discrete cards reach a smaller share of peak bandwidth than unified
    // memory does (measured 0.62 CUDA / 0.58 Vulkan against a predicted 0.8).
    if matches!(backend, Backend::Cuda | Backend::Vulkan) {
        efficiency *= 0.75;
    }
    let tokens_per_sec = (chip.bandwidth_gbps * 1e9 * efficiency / bytes_per_token.max(1.0)).min(250.0);

    // Prompt processing is where the backends diverge, and by a lot. CUDA's
    // measured 9,827 tok/s sat above the old 5,000 ceiling, so the ceiling was
    // understating NVIDIA; Vulkan's 251 tok/s is 39x slower, so the same
    // formula was overstating every AMD and Intel GPU by more than an order of
    // magnitude. A 10,000-token document is about a second on CUDA and about
    // forty on Vulkan -- a difference the user must be told about rather than
    // discover.
    let prompt_ceiling = match backend {
        Backend::Cuda => 12_000.0,
        Backend::Metal => 5_000.0,
        Backend::Vulkan => 400.0,
        Backend::Cpu => 60.0,
    };
    let prompt_scale = match backend {
        Backend::Vulkan => 0.026,   // 251 / 9827, measured on the same card
        Backend::Cpu => 0.004,
        _ => 1.0,
    };
    let prompt_per_sec =
        (chip.gpu_tflops * 1e12 * 0.9 * prompt_scale / (2.0 * active * 1e9)).clamp(5.0, prompt_ceiling);
    let reply_secs = TYPICAL_PROMPT / prompt_per_sec + TYPICAL_ANSWER / tokens_per_sec;
    SpeedEstimate {
        tokens_per_sec,
        prompt_per_sec,
        reply_secs,
        reply_thinking_secs: reply_secs + TYPICAL_THINKING / tokens_per_sec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_chips() {
        let m4 = identify("Apple M4", Some(10));
        assert_eq!((m4.generation, m4.tier, m4.bandwidth_gbps), (4, Tier::Base, 120.0));
        assert!(m4.exact);
        assert_eq!(identify("Apple M2 Pro", None).bandwidth_gbps, 200.0);
        assert_eq!(identify("Apple M4 Max", Some(32)).bandwidth_gbps, 410.0);
        assert_eq!(identify("Apple M4 Max", Some(40)).bandwidth_gbps, 546.0);
        assert_eq!(identify("Apple M3 Max", Some(30)).bandwidth_gbps, 300.0);
        assert_eq!(identify("Apple M1 Ultra", None).tier, Tier::Ultra);
        let intel = identify("Intel(R) Core(TM) i7", None);
        assert_eq!(intel.generation, 0);
        assert!(!intel.exact);
        let future = identify("Apple M6 Pro", None);
        assert!(!future.exact && future.bandwidth_gbps > 200.0);
    }

    #[test]
    fn m4_is_faster_than_m2_and_pro_faster_than_base() {
        let file = 7_460_000_000; // Qwen3.5 9B Q6_K
        let m2 = estimate(&identify("Apple M2", None), file, Some(9.2), None);
        let m4 = estimate(&identify("Apple M4", None), file, Some(9.2), None);
        let m4p = estimate(&identify("Apple M4 Pro", None), file, Some(9.2), None);
        assert!(m4.tokens_per_sec > m2.tokens_per_sec);
        assert!(m4p.tokens_per_sec > 2.0 * m4.tokens_per_sec);
        assert!(m4.reply_secs < m2.reply_secs);
    }

    #[test]
    fn estimates_are_realistic() {
        let m4 = identify("Apple M4", Some(10));
        // An 8B Q4_K_M (~4.9 GB) runs around 20 tok/s on a base M4.
        let s = estimate(&m4, 4_900_000_000, Some(8.2), None);
        assert!((15.0..=25.0).contains(&s.tokens_per_sec), "{s:?}");
        // MoE with 3B active out of 30B is several times faster than its size suggests.
        let moe = estimate(&m4, 18_000_000_000, Some(30.5), Some(3.0));
        let dense = estimate(&m4, 18_000_000_000, Some(30.5), None);
        assert!(moe.tokens_per_sec > 3.0 * dense.tokens_per_sec, "{moe:?} {dense:?}");
        assert!(s.reply_thinking_secs > s.reply_secs);
    }
}
