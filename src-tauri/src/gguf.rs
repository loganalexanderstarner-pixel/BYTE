//! Reads the metadata at the start of a GGUF model file (the header llama.cpp uses):
//! architecture, size, context length and the numbers BYTE's memory planner needs.
//! Only the header is read (the first few MB), so it works on a download's first
//! bytes too. Same rules as scripts/build-catalog.mjs `archFrom`.

use std::collections::HashMap;

use crate::system::ModelArch;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Numeric arrays (e.g. per-layer KV heads); string arrays aren't kept.
    Ints(Vec<i64>),
    Other,
}

impl Val {
    pub fn int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Ints(v) => v.iter().copied().max(),
            Val::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        if let Val::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ReadError {
    /// More bytes are needed to finish the header.
    Short,
    Bad(String),
}

/// The header's key/value pairs.
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub kv: HashMap<String, Val>,
}

struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ReadError> {
        let end = self.at.checked_add(n).ok_or_else(|| ReadError::Bad("bad length".into()))?;
        if end > self.b.len() {
            return Err(ReadError::Short);
        }
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, ReadError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, ReadError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, ReadError> {
        let n = self.u64()?;
        if n > 64 * 1024 * 1024 {
            return Err(ReadError::Bad("string too long".into()));
        }
        Ok(String::from_utf8_lossy(self.take(n as usize)?).into_owned())
    }
    /// One value of type `t` (numbers are kept; strings too unless `keep_str` is false).
    fn value(&mut self, t: u32, keep_str: bool) -> Result<Val, ReadError> {
        Ok(match t {
            0 => Val::Int(self.take(1)?[0] as i64),
            1 => Val::Int(self.take(1)?[0] as i8 as i64),
            2 => Val::Int(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            3 => Val::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            4 => Val::Int(self.u32()? as i64),
            5 => Val::Int(self.u32()? as i32 as i64),
            6 => Val::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            7 => Val::Bool(self.take(1)?[0] != 0),
            8 => {
                let s = self.string()?;
                if keep_str {
                    Val::Str(s)
                } else {
                    Val::Other
                }
            }
            9 => {
                let et = self.u32()?;
                let n = self.u64()?;
                if n > 100_000_000 {
                    return Err(ReadError::Bad("array too long".into()));
                }
                let mut ints = Vec::new();
                for _ in 0..n {
                    match self.value(et, false)? {
                        Val::Int(i) if ints.len() < 4096 => ints.push(i),
                        _ => {}
                    }
                }
                if ints.is_empty() {
                    Val::Other
                } else {
                    Val::Ints(ints)
                }
            }
            10 => Val::Int(self.u64()? as i64),
            11 => Val::Int(self.u64()? as i64),
            12 => Val::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            t => return Err(ReadError::Bad(format!("unknown value type {t}"))),
        })
    }
}

/// Reads the header from the start of a GGUF file.
pub fn parse(bytes: &[u8]) -> Result<Meta, ReadError> {
    let mut c = Cur { b: bytes, at: 0 };
    if c.take(4)? != b"GGUF" {
        return Err(ReadError::Bad("this isn't a GGUF model file".into()));
    }
    let version = c.u32()?;
    if version < 2 {
        return Err(ReadError::Bad(format!("GGUF version {version} is too old")));
    }
    let _tensors = c.u64()?;
    let n = c.u64()?;
    if n > 100_000 {
        return Err(ReadError::Bad("too many header entries".into()));
    }
    let mut kv = HashMap::new();
    for _ in 0..n {
        let key = c.string()?;
        let t = c.u32()?;
        // The tokenizer's vocabulary is huge and not needed; the chat template is.
        let keep = !key.starts_with("tokenizer.ggml.");
        let v = c.value(t, keep)?;
        kv.insert(key, v);
    }
    Ok(Meta { kv })
}

impl Meta {
    pub fn get(&self, k: &str) -> Option<&Val> {
        self.kv.get(k)
    }

    pub fn architecture(&self) -> String {
        self.get("general.architecture").and_then(Val::str).unwrap_or("unknown").to_string()
    }

    fn arch_int(&self, k: &str) -> Option<i64> {
        self.get(&format!("{}.{k}", self.architecture())).and_then(Val::int)
    }

    /// The numbers the memory planner uses (like build-catalog.mjs `archFrom`).
    pub fn arch(&self) -> ModelArch {
        let a = self.architecture();
        let n_layer = self.arch_int("block_count").unwrap_or(0) as u32;
        let max_ctx = self.arch_int("context_length").unwrap_or(8192).clamp(512, 4_194_304) as u32;
        let heads = self.arch_int("attention.head_count").unwrap_or(0);
        if heads == 0 {
            return ModelArch { n_layer, kv_layers: 0, n_head_kv: 0, head_dim: 0, max_ctx };
        }
        let kv_val = self.get(&format!("{a}.attention.head_count_kv"));
        let n_head_kv = kv_val.and_then(Val::int).unwrap_or(heads) as u32;
        let key_len = self.arch_int("attention.key_length").unwrap_or_else(|| self.arch_int("embedding_length").unwrap_or(0) / heads.max(1));
        let val_len = self.arch_int("attention.value_length").unwrap_or(key_len);
        let mut kv_layers = n_layer;
        if let Some(Val::Ints(v)) = kv_val {
            kv_layers = v.iter().filter(|x| **x > 0).count() as u32;
        }
        if let Some(i) = self.arch_int("full_attention_interval").filter(|i| *i > 1) {
            kv_layers = n_layer.div_ceil(i as u32);
        }
        ModelArch { n_layer, kv_layers, n_head_kv, head_dim: ((key_len + val_len) / 2) as u32, max_ctx }
    }

    /// The quantization, from `general.file_type` (llama.cpp's LLAMA_FTYPE numbers).
    pub fn quant(&self) -> Option<&'static str> {
        let t = self.get("general.file_type").and_then(Val::int)?;
        Some(match t {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            7 => "Q8_0",
            8 => "Q5_0",
            9 => "Q5_1",
            10 => "Q2_K",
            11 => "Q3_K_S",
            12 => "Q3_K_M",
            13 => "Q3_K_L",
            14 => "Q4_K_S",
            15 => "Q4_K_M",
            16 => "Q5_K_S",
            17 => "Q5_K_M",
            18 => "Q6_K",
            19 => "IQ2_XXS",
            20 => "IQ2_XS",
            21 => "Q2_K_S",
            22 => "IQ3_XS",
            23 => "IQ3_XXS",
            24 => "IQ1_S",
            25 => "IQ4_NL",
            26 => "IQ3_S",
            27 => "IQ3_M",
            28 => "IQ2_S",
            29 => "IQ2_M",
            30 => "IQ4_XS",
            31 => "IQ1_M",
            32 => "BF16",
            36 => "TQ1_0",
            37 => "TQ2_0",
            _ => return None,
        })
    }

    /// The model's name ("general.name"), if it has one.
    pub fn name(&self) -> Option<String> {
        self.get("general.name").and_then(Val::str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Billions of parameters: from `general.size_label` ("8B", "30B-A3B"), else estimated
    /// from the file size and the quantization's bits per weight.
    pub fn params_b(&self, file_bytes: u64) -> Option<f32> {
        if let Some(label) = self.get("general.size_label").and_then(Val::str) {
            let first = label.split(['-', 'x', 'X']).next().unwrap_or("");
            let (num, unit) = first.trim().split_at(first.trim().find(|c: char| c.is_ascii_alphabetic()).unwrap_or(first.len()));
            if let Ok(n) = num.parse::<f32>() {
                let b = match unit.to_ascii_uppercase().as_str() {
                    "B" => n,
                    "M" => n / 1000.0,
                    "T" => n * 1000.0,
                    _ => 0.0,
                };
                // "8x7B" (mixture of experts): the total.
                let times = label.split(['x', 'X']).next().filter(|_| label.contains(['x', 'X'])).and_then(|x| x.parse::<f32>().ok()).unwrap_or(1.0);
                if b > 0.0 {
                    return Some(b * if label.contains(['x', 'X']) { times.max(1.0) } else { 1.0 });
                }
            }
        }
        let bits = bits_of(self.quant()?);
        Some((file_bytes as f64 * 8.0 / bits / 1e9) as f32).filter(|b| *b > 0.05)
    }

    /// The chat template can think (Qwen3-style `enable_thinking`, or `<think>`).
    pub fn thinks(&self) -> bool {
        self.get("tokenizer.chat_template").and_then(Val::str).is_some_and(|t| t.contains("enable_thinking") || t.contains("<think>"))
    }
}

/// Approximate bits per weight of a quantization (for size estimates).
pub fn bits_of(quant: &str) -> f64 {
    match quant {
        "F32" => 32.0,
        "F16" | "BF16" => 16.0,
        "Q8_0" => 8.5,
        "Q6_K" => 6.6,
        "Q5_K_M" | "Q5_K_S" | "Q5_0" | "Q5_1" => 5.6,
        "Q4_K_M" | "Q4_1" => 4.85,
        "Q4_K_S" | "Q4_0" | "IQ4_NL" | "IQ4_XS" => 4.5,
        "Q3_K_L" | "Q3_K_M" | "IQ3_M" | "IQ3_S" => 3.9,
        "Q3_K_S" | "IQ3_XS" | "IQ3_XXS" => 3.4,
        "Q2_K" | "Q2_K_S" | "IQ2_M" | "IQ2_S" => 2.9,
        "IQ2_XS" | "IQ2_XXS" => 2.3,
        "IQ1_S" | "IQ1_M" | "TQ1_0" | "TQ2_0" => 1.8,
        _ => 4.85,
    }
}

/// Builds a small GGUF header for tests.
#[cfg(test)]
pub fn test_header(kv: &[(&str, Val)]) -> Vec<u8> {
    let mut b = b"GGUF".to_vec();
    b.extend(3u32.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend((kv.len() as u64).to_le_bytes());
    let s = |b: &mut Vec<u8>, t: &str| {
        b.extend((t.len() as u64).to_le_bytes());
        b.extend(t.as_bytes());
    };
    for (k, v) in kv {
        s(&mut b, k);
        match v {
            Val::Int(i) => {
                b.extend(4u32.to_le_bytes());
                b.extend((*i as u32).to_le_bytes());
            }
            Val::Str(t) => {
                b.extend(8u32.to_le_bytes());
                s(&mut b, t);
            }
            Val::Ints(xs) => {
                b.extend(9u32.to_le_bytes());
                b.extend(4u32.to_le_bytes());
                b.extend((xs.len() as u64).to_le_bytes());
                for x in xs {
                    b.extend((*x as u32).to_le_bytes());
                }
            }
            Val::Bool(x) => {
                b.extend(7u32.to_le_bytes());
                b.push(*x as u8);
            }
            _ => unreachable!(),
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qwen() -> Vec<u8> {
        test_header(&[
            ("general.architecture", Val::Str("qwen3".into())),
            ("general.name", Val::Str("Qwen3 8B".into())),
            ("general.size_label", Val::Str("8B".into())),
            ("general.file_type", Val::Int(15)),
            ("qwen3.block_count", Val::Int(36)),
            ("qwen3.context_length", Val::Int(40960)),
            ("qwen3.embedding_length", Val::Int(4096)),
            ("qwen3.attention.head_count", Val::Int(32)),
            ("qwen3.attention.head_count_kv", Val::Int(8)),
            ("qwen3.attention.key_length", Val::Int(128)),
            ("tokenizer.chat_template", Val::Str("{% if enable_thinking %}<think>{% endif %}".into())),
        ])
    }

    #[test]
    fn reads_a_header() {
        let m = parse(&qwen()).unwrap();
        assert_eq!(m.architecture(), "qwen3");
        assert_eq!(m.name().as_deref(), Some("Qwen3 8B"));
        assert_eq!(m.quant(), Some("Q4_K_M"));
        assert_eq!(m.params_b(5_000_000_000), Some(8.0));
        assert!(m.thinks());
        assert_eq!(m.arch(), ModelArch { n_layer: 36, kv_layers: 36, n_head_kv: 8, head_dim: 128, max_ctx: 40960 });
    }

    #[test]
    fn short_and_bad_input_is_told_apart() {
        let full = qwen();
        assert_eq!(parse(&full[..full.len() - 3]).unwrap_err(), ReadError::Short);
        assert!(matches!(parse(b"PK\x03\x04 not a model"), Err(ReadError::Bad(_))));
    }

    #[test]
    fn hybrid_models_cache_fewer_layers_and_sizes_are_estimated() {
        let h = test_header(&[
            ("general.architecture", Val::Str("lfm2".into())),
            ("general.file_type", Val::Int(7)),
            ("lfm2.block_count", Val::Int(16)),
            ("lfm2.attention.head_count", Val::Int(16)),
            ("lfm2.attention.head_count_kv", Val::Ints(vec![0, 0, 8, 0, 8, 0, 0, 8, 0, 0, 8, 0, 8, 0, 8, 0])),
            ("lfm2.embedding_length", Val::Int(2048)),
        ]);
        let m = parse(&h).unwrap();
        let a = m.arch();
        assert_eq!((a.kv_layers, a.n_head_kv, a.head_dim, a.max_ctx), (6, 8, 128, 8192));
        // No size label: 1.25 GB at Q8_0 (~8.5 bits) is about 1.2B parameters.
        let b = m.params_b(1_250_000_000).unwrap();
        assert!((1.1..1.3).contains(&b), "{b}");
    }
}
