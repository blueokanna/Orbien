use nextjson::{NsonDeserialize, NsonSerialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, NsonSerialize, NsonDeserialize)]
#[njson(rename_all = "snake_case")]
pub enum ReloadLevel {
    #[default]
    Noop,
    TunnelsOnly,
    ReconnectControl,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize)]
pub struct ReloadOutcome {
    #[njson(default)]
    pub level: ReloadLevel,
    #[njson(default)]
    pub added: Vec<String>,
    #[njson(default)]
    pub removed: Vec<String>,
    #[njson(default)]
    pub updated: Vec<String>,
    #[njson(default)]
    pub failed: Vec<(String, String)>,
    #[njson(default)]
    pub connection_settings_changed: bool,
}

impl ReloadLevel {
    pub fn label(self) -> &'static str {
        match self {
            ReloadLevel::Noop => "noop",
            ReloadLevel::TunnelsOnly => "tunnels",
            ReloadLevel::ReconnectControl => "reconnect",
        }
    }
}

impl ReloadOutcome {
    pub fn succeeded(&self) -> bool {
        self.failed.is_empty()
    }
}
