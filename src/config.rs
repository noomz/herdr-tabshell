use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Percent(u16),
    Cells(u16),
}

impl Size {
    fn parse(value: &str) -> Option<Self> {
        if let Some(percent) = value.strip_suffix('%') {
            let n = percent.parse::<u16>().ok()?;
            Some(Self::Percent(n.clamp(1, 100)))
        } else {
            let n = value.parse::<u16>().ok()?;
            (n > 0).then_some(Self::Cells(n))
        }
    }

    #[allow(dead_code)]
    pub fn to_json(self) -> Value {
        match self {
            Self::Percent(n) => Value::String(format!("{n}%")),
            Self::Cells(n) => Value::from(n),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub tab_windows: bool,
    pub width: Size,
    pub height: Size,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_windows: true,
            width: Size::Percent(80),
            height: Size::Percent(80),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let Some(dir) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") else {
            return Self::default();
        };
        let path = Path::new(&dir).join("tabshell.conf");
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("herdr-tabshell: {}: {error}", path.display());
                }
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                eprintln!("herdr-tabshell: ignoring invalid config line {line:?}");
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let parsed = match key {
                "tab_windows" => match value {
                    "true" | "yes" | "on" | "1" => Some(true),
                    "false" | "no" | "off" | "0" => Some(false),
                    _ => None,
                }
                .map(|enabled| config.tab_windows = enabled),
                "width" => Size::parse(value).map(|size| config.width = size),
                "height" => Size::parse(value).map(|size| config.height = size),
                _ => None,
            };
            if parsed.is_none() {
                eprintln!("herdr-tabshell: ignoring unknown or invalid setting {key}={value}");
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults() {
        let config = Config::default();
        assert!(config.tab_windows);
        assert_eq!(config.width, Size::Percent(80));
        assert_eq!(config.height, Size::Percent(80));
        assert_eq!(Config::parse(""), config);
    }

    #[test]
    fn width_key() {
        let config = Config::parse("width = 60%");
        assert_eq!(config.width, Size::Percent(60));
        assert_eq!(config.height, Size::Percent(80));
    }

    #[test]
    fn height_key() {
        let config = Config::parse("height = 24");
        assert_eq!(config.height, Size::Cells(24));
        assert_eq!(config.width, Size::Percent(80));
    }

    #[test]
    fn tab_windows_false() {
        assert!(!Config::parse("tab_windows = false").tab_windows);
    }

    #[test]
    fn boolean_aliases() {
        for value in ["true", "yes", "on", "1"] {
            let config = Config::parse(&format!("tab_windows=false\ntab_windows={value}"));
            assert!(config.tab_windows, "{value}");
        }
        for value in ["false", "no", "off", "0"] {
            assert!(
                !Config::parse(&format!("tab_windows={value}")).tab_windows,
                "{value}"
            );
        }
    }

    #[test]
    fn percent_and_cells_have_distinct_json_values() {
        let config = Config::parse("width=80%\nheight=80");
        assert_eq!(config.width, Size::Percent(80));
        assert_eq!(config.height, Size::Cells(80));
        assert_eq!(config.width.to_json(), json!("80%"));
        assert_eq!(config.height.to_json(), json!(80));
    }

    #[test]
    fn percentages_clamp() {
        let config = Config::parse("width=0%\nheight=999%");
        assert_eq!(config.width, Size::Percent(1));
        assert_eq!(config.height, Size::Percent(100));
        let config = Config::parse("width=1%\nheight=100%");
        assert_eq!(config.width, Size::Percent(1));
        assert_eq!(config.height, Size::Percent(100));
    }

    #[test]
    fn cells_are_not_clamped_to_percentages() {
        let config = Config::parse("width=120\nheight=1");
        assert_eq!(config.width, Size::Cells(120));
        assert_eq!(config.height, Size::Cells(1));
    }

    #[test]
    fn bad_values_keep_defaults() {
        for value in ["", "banana", "-1", "-1%", "80%%", "1.5%", "65536"] {
            let config = Config::parse(&format!(
                "width={value}\nheight={value}\ntab_windows={value}"
            ));
            assert_eq!(config, Config::default(), "{value:?}");
        }
        assert_eq!(Config::parse("width=0\nheight=0"), Config::default());
    }

    #[test]
    fn bad_values_keep_previous_valid_settings() {
        let config = Config::parse(
            "width=50\nwidth=bad\nheight=60%\nheight=bad\ntab_windows=false\ntab_windows=bad",
        );
        assert_eq!(config.width, Size::Cells(50));
        assert_eq!(config.height, Size::Percent(60));
        assert!(!config.tab_windows);
    }

    #[test]
    fn comments_blank_lines_and_whitespace() {
        let config = Config::parse("  # config\n\n \t\n width = 70% # width\n height = 30 \n");
        assert_eq!(config.width, Size::Percent(70));
        assert_eq!(config.height, Size::Cells(30));
    }

    #[test]
    fn unknown_keys_and_malformed_lines_are_ignored() {
        assert_eq!(
            Config::parse("unknown=42\nnot a key value line"),
            Config::default()
        );
    }
}
