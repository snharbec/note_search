use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

pub struct MappingConfig {
    pub mappings: HashMap<String, String>,
}

impl MappingConfig {
    pub fn load() -> Self {
        let config_path = match env::var("NOTE_SEARCH_CONFIG") {
            Ok(p) => Path::new(&p).to_path_buf(),
            Err(_) => {
                let home_dir = env::var("HOME").unwrap_or_else(|_| ".".to_string());
                Path::new(&home_dir).join(".config/note_search/config")
            }
        };

        let mut mappings = HashMap::new();

        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                let mut current_section = String::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        current_section = trimmed[1..trimmed.len() - 1].to_string();
                    } else if current_section == "Mapping" && trimmed.contains('=') {
                        let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
                        if parts.len() == 2 {
                            mappings
                                .insert(parts[0].trim().to_string(), parts[1].trim().to_string());
                        }
                    }
                }
            }
        }

        Self { mappings }
    }

    pub fn get(&self, key: &str) -> String {
        self.mappings
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    pub fn apply_to_attributes(&self, attributes: &mut HashMap<String, serde_json::Value>) {
        let keys: Vec<String> = attributes.keys().cloned().collect();
        let mut to_merge: Vec<(String, serde_json::Value)> = Vec::new();

        for key in keys {
            if let Some(target) = self.mappings.get(&key) {
                if target != &key {
                    if let Some(val) = attributes.remove(&key) {
                        to_merge.push((target.clone(), val));
                    }
                }
            }
        }

        for (target, val) in to_merge {
            let entry = attributes
                .entry(target.clone())
                .or_insert(serde_json::Value::Array(Vec::new()));

            if !entry.is_array() {
                let old_val = entry.clone();
                *entry = serde_json::Value::Array(vec![old_val]);
            }

            if let Some(arr) = entry.as_array_mut() {
                match val {
                    serde_json::Value::Array(items) => {
                        for item in items {
                            if !arr.contains(&item) {
                                arr.push(item);
                            }
                        }
                    }
                    single => {
                        if !arr.contains(&single) {
                            arr.push(single);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_mapping_config_load() {
        // Just verifies that load() doesn't panic and returns a valid struct
        let config = MappingConfig::load();
        // We don't assert specific values since they depend on the user's actual config
        assert!(config.mappings.len() >= 0);
    }

    #[test]
    fn test_mapping_get_existing() {
        let mut config = MappingConfig {
            mappings: HashMap::new(),
        };
        config
            .mappings
            .insert("participant".to_string(), "people".to_string());
        assert_eq!(config.get("participant"), "people");
    }

    #[test]
    fn test_mapping_get_unknown() {
        let config = MappingConfig {
            mappings: HashMap::new(),
        };
        assert_eq!(config.get("unknown"), "unknown");
    }

    #[test]
    fn test_apply_to_attributes_simple() {
        let mut config = MappingConfig {
            mappings: HashMap::new(),
        };
        config
            .mappings
            .insert("participant".to_string(), "people".to_string());

        let mut attrs = HashMap::new();
        attrs.insert("participant".to_string(), serde_json::json!("Alice"));

        config.apply_to_attributes(&mut attrs);

        assert!(!attrs.contains_key("participant"));
        assert_eq!(attrs.get("people"), Some(&serde_json::json!(["Alice"])));
    }

    #[test]
    fn test_apply_to_attributes_array_merge() {
        let mut config = MappingConfig {
            mappings: HashMap::new(),
        };
        config
            .mappings
            .insert("participant".to_string(), "people".to_string());
        config
            .mappings
            .insert("participants".to_string(), "people".to_string());

        let mut attrs = HashMap::new();
        attrs.insert("participant".to_string(), serde_json::json!("Alice"));
        attrs.insert(
            "participants".to_string(),
            serde_json::json!(["Bob", "Charlie"]),
        );
        attrs.insert("people".to_string(), serde_json::json!("Dave"));

        config.apply_to_attributes(&mut attrs);

        assert!(!attrs.contains_key("participant"));
        assert!(!attrs.contains_key("participants"));

        let people = attrs.get("people").unwrap();
        let arr = people.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert!(arr.contains(&serde_json::json!("Alice")));
        assert!(arr.contains(&serde_json::json!("Bob")));
        assert!(arr.contains(&serde_json::json!("Charlie")));
        assert!(arr.contains(&serde_json::json!("Dave")));
    }

    #[test]
    fn test_load_from_temp_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config");
        let mut file = fs::File::create(&config_path).unwrap();
        writeln!(file, "[Mapping]").unwrap();
        writeln!(file, "foo=bar").unwrap();
        writeln!(file, "baz=qux").unwrap();
        drop(file);

        env::set_var("NOTE_SEARCH_CONFIG", config_path.to_str().unwrap());
        let config = MappingConfig::load();

        assert_eq!(config.get("foo"), "bar");
        assert_eq!(config.get("baz"), "qux");

        env::remove_var("NOTE_SEARCH_CONFIG");
    }
}
