pub fn greet(name: &str) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::greet;

    #[test]
    fn greeting_is_exact() {
        assert_eq!(greet("Alcatraz"), "Hello, Alcatraz!");
    }
}
