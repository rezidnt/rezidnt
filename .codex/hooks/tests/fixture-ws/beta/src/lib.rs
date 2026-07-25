//! A failing test in a SECOND crate, so --no-fail-fast produces multiple roster blocks.
pub fn v() -> u8 {
    4
}

#[cfg(test)]
mod tests {
    #[test]
    fn beta_fold_mismatch() {
        assert_eq!(super::v(), 5, "deliberate: fixture failure");
    }
}
