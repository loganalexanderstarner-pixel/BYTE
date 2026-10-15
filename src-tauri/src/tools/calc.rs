//! Exact calculator (arithmetic, units, percentages, dates) so BYTE never does
//! math "in its head". Backed by fend.

use crate::error::{AppError, AppResult};

pub fn calculate(expression: &str) -> AppResult<String> {
    let expr = expression.trim();
    if expr.is_empty() {
        return Err(AppError::msg("empty expression"));
    }
    if expr.len() > 500 {
        return Err(AppError::msg("expression is too long"));
    }
    let mut ctx = fend_core::Context::new();
    // Deterministic: no randomness, no network lookups.
    ctx.set_random_u32_fn(|| 4);
    let result = fend_core::evaluate(expr, &mut ctx).map_err(|e| AppError::msg(format!("can't calculate that: {e}")))?;
    let out = result.get_main_result().trim().to_string();
    if out.is_empty() {
        return Err(AppError::msg("the expression produced no result"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(calculate("17*23 + 4^2").unwrap(), "407");
        assert_eq!(calculate("0.1 + 0.2").unwrap(), "0.3");
    }

    #[test]
    fn units_and_percentages() {
        assert_eq!(calculate("5 km to miles").unwrap().split_whitespace().last(), Some("miles"));
        assert!(calculate("15% of 80").unwrap().starts_with("12"));
    }

    #[test]
    fn errors_are_reported() {
        assert!(calculate("").is_err());
        assert!(calculate("1 / (").is_err());
    }
}
