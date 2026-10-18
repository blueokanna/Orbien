use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogConfig {
    #[serde(default)]
    pub file: String,

    #[serde(default = "default_level")]
    pub level: String,

    #[serde(default = "default_max_days", rename = "maxDays", alias = "max_days")]
    pub max_days: u32,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            file: String::new(),
            level: default_level(),
            max_days: default_max_days(),
        }
    }
}

impl LogConfig {
    pub fn complete(&mut self) {
        self.file = self.file.trim().to_string();
        let level = self.level.trim().to_ascii_lowercase();
        self.level = if level.is_empty() {
            default_level()
        } else {
            level
        };
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !is_valid_level(&self.level) {
            return Err(anyhow!(
                "log.level must be one of trace/debug/info/warn/error, got {:?}",
                self.level
            ));
        }
        Ok(())
    }

    pub fn file_enabled(&self) -> bool {
        !self.file.is_empty()
    }
}

fn default_level() -> String {
    "info".into()
}

fn default_max_days() -> u32 {
    3
}

fn is_valid_level(level: &str) -> bool {
    matches!(
        level.trim().to_ascii_lowercase().as_str(),
        "trace" | "debug" | "info" | "warn" | "error"
    )
}
