fn hello() -> &'static str {
    "test"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_returns_test() {
        assert_eq!(hello(), "test");
    }

    #[test]
    fn test_hello_is_static() {
        let s = hello();
        assert!(s.len() > 0);
    }
}
