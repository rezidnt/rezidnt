//! Two failing tests and one passing one, in the first crate cargo reaches.
pub fn v() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn alpha_green() {
        assert_eq!(super::v(), 1);
    }
    #[test]
    fn alpha_chain_mismatch() {
        assert_eq!(super::v(), 2, "deliberate: fixture failure");
    }
    #[test]
    fn alpha_rebuild_diverges() {
        assert_eq!(super::v(), 3, "deliberate: fixture failure");
    }
}
