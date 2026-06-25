#[derive(Debug, Clone, PartialEq)]
pub struct AttributePair {
    pub key: String,
    pub value: String,
}

impl AttributePair {
    pub fn new(key: &str, value: &str) -> Self {
        AttributePair {
            key: key.trim().to_string(),
            value: value.trim().to_string(),
        }
    }
}

impl std::fmt::Display for AttributePair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructor_and_getters() {
        let pair = AttributePair::new("key", "value");
        assert_eq!(pair.key, "key");
        assert_eq!(pair.value, "value");
    }

    #[test]
    fn test_trimming() {
        let pair = AttributePair::new("  key  ", "  value  ");
        assert_eq!(pair.key, "key");
        assert_eq!(pair.value, "value");
    }

    #[test]
    fn test_empty_key() {
        let pair = AttributePair::new("", "value");
        assert_eq!(pair.key, "");
        assert_eq!(pair.value, "value");
    }

    #[test]
    fn test_empty_value() {
        let pair = AttributePair::new("key", "");
        assert_eq!(pair.key, "key");
        assert_eq!(pair.value, "");
    }

    #[test]
    fn test_equality() {
        let pair1 = AttributePair::new("key", "value");
        let pair2 = AttributePair::new("key", "value");
        assert_eq!(pair1, pair2);
    }

    #[test]
    fn test_inequality_different_key() {
        let pair1 = AttributePair::new("key1", "value");
        let pair2 = AttributePair::new("key2", "value");
        assert_ne!(pair1, pair2);
    }

    #[test]
    fn test_inequality_different_value() {
        let pair1 = AttributePair::new("key", "value1");
        let pair2 = AttributePair::new("key", "value2");
        assert_ne!(pair1, pair2);
    }

    #[test]
    fn test_to_string() {
        let pair = AttributePair::new("key", "value");
        assert_eq!(pair.to_string(), "key=value");
    }

    #[test]
    fn test_to_string_with_spaces() {
        let pair = AttributePair::new("my key", "my value");
        assert_eq!(pair.to_string(), "my key=my value");
    }

    #[test]
    fn test_clone() {
        let pair1 = AttributePair::new("key", "value");
        let pair2 = pair1.clone();
        assert_eq!(pair1, pair2);
    }

    #[test]
    fn test_special_characters() {
        let pair = AttributePair::new("key-with-dash", "value_with_underscore");
        assert_eq!(pair.key, "key-with-dash");
        assert_eq!(pair.value, "value_with_underscore");
    }

    #[test]
    fn test_unicode() {
        let pair = AttributePair::new("键", "值");
        assert_eq!(pair.key, "键");
        assert_eq!(pair.value, "值");
    }
}
