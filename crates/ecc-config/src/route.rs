use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteTarget {
    pub provider: String,
    pub model: String,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteEntry {
    pub targets: Vec<RouteTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RouteTable {
    #[serde(default)]
    pub routes: HashMap<String, RouteEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
}

pub fn load_routes(path: &Path) -> Result<RouteTable, RouteError> {
    let content = std::fs::read_to_string(path)?;
    let table: RouteTable = toml::from_str(&content)?;
    Ok(table)
}

pub fn save_routes(path: &Path, table: &RouteTable) -> Result<(), RouteError> {
    let content = toml::to_string_pretty(table)?;
    // Atomic write via temp file + rename
    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn resolve_route<'a>(table: &'a RouteTable, model: &str) -> Option<&'a RouteEntry> {
    if let Some(entry) = table.routes.get(model) {
        return Some(entry);
    }
    // Date suffix fallback: claude-haiku-4-5-20251001 -> claude-haiku-4-5
    let stripped = strip_date_suffix(model);
    if stripped != model {
        return table.routes.get(stripped);
    }
    None
}

fn strip_date_suffix(model: &str) -> &str {
    let re = Regex::new(r"-\d{8}$").unwrap();
    match re.find(model) {
        Some(mat) if mat.start() > 0 => &model[..mat.start()],
        _ => model,
    }
}

impl RouteTable {
    pub fn from_str(content: &str) -> Result<Self, RouteError> {
        Ok(toml::from_str(content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_toml() -> &'static str {
        r#"
[routes."claude-sonnet-4-6"]
targets = [
  { provider = "kimi", model = "K2.6", priority = 1 },
  { provider = "zhipu", model = "glm-4", priority = 2 },
]

[routes."claude-haiku-4-5"]
targets = [
  { provider = "deepseek", model = "deepseek-chat", priority = 1 },
]
"#
    }

    #[test]
    fn t1_parse_route_with_priority_list() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        let entry = table.routes.get("claude-sonnet-4-6").unwrap();
        assert_eq!(entry.targets.len(), 2);
        assert_eq!(entry.targets[0].provider, "kimi");
        assert_eq!(entry.targets[0].model, "K2.6");
        assert_eq!(entry.targets[0].priority, 1);
        assert_eq!(entry.targets[1].provider, "zhipu");
        assert_eq!(entry.targets[1].model, "glm-4");
        assert_eq!(entry.targets[1].priority, 2);
    }

    #[test]
    fn t2_parse_empty_route_table() {
        let table = RouteTable::from_str("").unwrap();
        assert!(table.routes.is_empty());
    }

    #[test]
    fn t3_targets_sorted_by_priority() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        let entry = table.routes.get("claude-sonnet-4-6").unwrap();
        let priorities: Vec<u8> = entry.targets.iter().map(|t| t.priority).collect();
        let mut sorted = priorities.clone();
        sorted.sort();
        assert_eq!(priorities, sorted);
    }

    #[test]
    fn t4_round_trip_toml() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        let serialized = toml::to_string_pretty(&table).unwrap();
        let deserialized: RouteTable = toml::from_str(&serialized).unwrap();
        assert_eq!(table, deserialized);
    }

    #[test]
    fn t5_date_suffix_fallback() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        let entry = resolve_route(&table, "claude-haiku-4-5-20251001").unwrap();
        assert_eq!(entry.targets[0].provider, "deepseek");
    }

    #[test]
    fn t5_resolve_exact_match() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        let entry = resolve_route(&table, "claude-sonnet-4-6").unwrap();
        assert_eq!(entry.targets[0].provider, "kimi");
    }

    #[test]
    fn t5_resolve_no_match() {
        let table = RouteTable::from_str(sample_toml()).unwrap();
        assert!(resolve_route(&table, "claude-opus-4-7").is_none());
    }

    #[test]
    fn t_save_and_load_routes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routes.toml");
        let mut table = RouteTable::default();
        table.routes.insert(
            "claude-sonnet-4-6".to_string(),
            RouteEntry {
                targets: vec![RouteTarget {
                    provider: "kimi".to_string(),
                    model: "K2.6".to_string(),
                    priority: 1,
                }],
            },
        );
        save_routes(&path, &table).unwrap();
        let loaded = load_routes(&path).unwrap();
        assert_eq!(table, loaded);
    }
}
