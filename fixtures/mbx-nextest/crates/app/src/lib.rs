pub fn ok() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn is_ok() {
        assert!(super::ok());
    }
}
