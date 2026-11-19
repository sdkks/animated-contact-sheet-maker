// src/config.rs
// Resolves ~/.acsm.toml (or $ACSM_CONFIG), auto-creates a
// commented template on first run, parses it into a Config with
// Option<T> fields, and warns on unknown keys.

use acsm::{auto_layout::{self, LayoutRule, LayoutTable}, defaults};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ConfigError {
    ParseFailed { path: PathBuf, msg: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { path, msg } => {
                write!(f, "config parse failed: {}: {}", path.display(), msg)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

// Cfg fields are feature-surface for users; not every field is consulted by
// every subcommand (e.g., screenshots doesn't read ReelCfg).
#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub screenshots: ScreenshotsCfg,
    #[serde(default)]
    pub sheet: SheetCfg,
    #[serde(default)]
    pub reel: ReelCfg,
    #[serde(default)]
    pub animated_sheet: AnimatedSheetCfg,
    #[serde(default)]
    pub layout: LayoutCfg,
    #[serde(skip)]
    layout_rule_unknown: Vec<(usize, String)>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LayoutCfg {
    pub trim_start_secs: Option<u32>,
    pub trim_end_secs: Option<u32>,
    pub rules: Option<Vec<LayoutRule>>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct ScreenshotsCfg {
    pub count: Option<u32>,
    pub format: Option<String>,
    pub quality: Option<u32>,
    pub suffix: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct SheetCfg {
    #[serde(default)]
    pub auto: Option<bool>,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub width: Option<u32>,
    pub gap: Option<u32>,
    pub thumb_font: Option<u32>,
    pub header_font: Option<u32>,
    pub show_timestamps: Option<bool>,
    pub show_header: Option<bool>,
    pub format: Option<String>,
    pub quality: Option<u32>,
    pub theme: Option<String>,
    pub suffix: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct ReelCfg {
    pub count: Option<u32>,
    pub clip_length_secs: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub quality: Option<u32>,
    pub format: Option<String>,
    pub suffix: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct AnimatedSheetCfg {
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub auto: Option<bool>,
    pub width: Option<u32>,
    pub gap: Option<u32>,
    pub clip_length_secs: Option<u32>,
    pub fps: Option<u32>,
    pub quality: Option<u32>,
    pub compression_effort: Option<u32>,
    pub thumb_font: Option<u32>,
    pub header_font: Option<u32>,
    pub show_timestamps: Option<bool>,
    pub show_header: Option<bool>,
    pub theme: Option<String>,
    pub suffix: Option<String>,
}

/// Default path: `$HOME/.acsm.toml` (Unix) or
/// `%USERPROFILE%\.acsm.toml` (Windows). Overridable via
/// `$ACSM_CONFIG`. Returns `None` when no home is set (sandboxed CI).
/// The bool flag is `true` when the path came from `$ACSM_CONFIG`
/// (user explicitly set it) — callers use this to decide whether a
/// write-fail on auto-create warrants a warning.
pub fn resolve_path() -> Option<(PathBuf, bool)> {
    if let Ok(p) = std::env::var("ACSM_CONFIG") {
        return Some((PathBuf::from(p), true));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some((PathBuf::from(home).join(".acsm.toml"), false))
}

/// Load config from `path` if it exists, otherwise create a commented
/// template (printing a one-time notice to stderr) and return defaults.
/// On any I/O error during creation, skip silently and return defaults.
/// Returns `Err(ConfigError)` if the file exists but cannot be parsed.
/// `is_explicit` should be `true` when the path came from `$ACSM_CONFIG`
/// so that a failed write emits a warning rather than failing silently.
pub fn load_or_create(path: &Path, is_explicit: bool) -> Result<Config, ConfigError> {
    if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(body) => return parse(&body, path),
            Err(e) => {
                eprintln!("warning: could not read {}: {e}", path.display());
                return Ok(Config::default());
            }
        }
    }
    match std::fs::write(path, template()) {
        Ok(()) => eprintln!("Created {}", path.display()),
        Err(e) => {
            if is_explicit {
                eprintln!("warning: ACSM_CONFIG={} could not be created: {e}", path.display());
            }
            // read-only $HOME / sandbox: silently fall back to defaults
        }
    }
    Ok(Config::default())
}

fn parse(body: &str, path: &Path) -> Result<Config, ConfigError> {
    // A permissive document pass retains keys that serde otherwise ignores.
    let raw = toml::from_str::<toml::Table>(body).ok();
    if let Some(raw) = &raw {
        for warning in warn_unknown_keys(raw, path) { eprintln!("{warning}"); }
    }
    let mut config = toml::from_str::<Config>(body).map_err(|e| ConfigError::ParseFailed {
        path: path.to_path_buf(),
        msg: e.to_string(),
    })?;
    if let Some(raw) = &raw { config.layout_rule_unknown = scan_layout_rule_keys(raw); }
    Ok(config)
}

fn scan_layout_rule_keys(raw: &toml::Table) -> Vec<(usize, String)> {
    let mut unknown = Vec::new();
    if let Some(rules) = raw.get("layout").and_then(|v| v.as_table())
        .and_then(|t| t.get("rules")).and_then(|v| v.as_array()) {
        for (i, value) in rules.iter().enumerate() {
            if let Some(rule) = value.as_table() {
                for key in rule.keys() {
                    if !auto_layout::RULE_KEYS.contains(&key.as_str()) {
                        unknown.push((i + 1, key.clone()));
                    }
                }
            }
        }
    }
    unknown
}

fn warn_unknown_keys(raw: &toml::Table, path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let known_sections = ["screenshots", "sheet", "reel", "animated_sheet", "layout"];
    let known_keys: &[(&str, &[&str])] = &[
        ("screenshots", &["count", "format", "quality", "suffix"]),
        (
            "sheet",
            &[
                "auto",
                "cols",
                "rows",
                "width",
                "gap",
                "thumb_font",
                "header_font",
                "show_timestamps",
                "show_header",
                "format",
                "quality",
                "theme",
                "suffix",
            ],
        ),
        (
            "reel",
            &[
                "count",
                "clip_length_secs",
                "height",
                "fps",
                "quality",
                "format",
                "suffix",
            ],
        ),
        (
            "animated_sheet",
            &[
                "cols",
                "rows",
                "auto",
                "width",
                "gap",
                "clip_length_secs",
                "fps",
                "quality",
                "compression_effort",
                "thumb_font",
                "header_font",
                "show_timestamps",
                "show_header",
                "theme",
                "suffix",
            ],
        ),
        ("layout", &["trim_start_secs", "trim_end_secs", "rules"]),
    ];
    for (section, value) in raw {
        if !known_sections.contains(&section.as_str()) {
            warnings.push(format!("warning: unknown section '{section}' in {}", path.display()));
            continue;
        }
        let Some(sub) = value.as_table() else {
            continue;
        };
        let allowed = known_keys
            .iter()
            .find(|(s, _)| *s == section)
            .map(|(_, k)| *k)
            .unwrap_or(&[]);
        for key in sub.keys() {
            if !allowed.contains(&key.as_str()) {
                warnings.push(format!("warning: unknown key '{section}.{key}' in {}", path.display()));
            }
        }
    }
    warnings
}

fn template() -> &'static str {
    // Keep in sync with the Cfg structs above. Every field appears commented
    // so users can uncomment and edit individual values.
    concat!(
        "# ~/.acsm.toml — per-user defaults for acsm.\n",
        "# Uncomment any key to override the built-in default.\n",
        "# CLI flags always override this file.\n",
        "\n",
        "[screenshots]\n",
        "# count = 8\n",
        "# format = \"png\"        # png | jpeg\n",
        "# quality = 92           # 50-100, JPEG only\n",
        "# suffix = \"_screens_\"\n",
        "\n",
        "[sheet]\n",
        "# cols = 3             # ignored in auto mode\n",
        "# rows = 6             # ignored in auto mode\n",
        "# auto = false         # duration-driven grid; CLI --cols/--rows override this\n",
        "# width = 1920\n",
        "# gap = 10\n",
        "# thumb_font = 18\n",
        "# header_font = 20\n",
        "# show_timestamps = true\n",
        "# show_header = true\n",
        "# format = \"png\"        # png | jpeg\n",
        "# quality = 92\n",
        "# theme = \"dark\"        # dark | light\n",
        "# suffix = \"_sheet\"\n",
        "\n",
        "[reel]\n",
        "# count = 15\n",
        "# clip_length_secs = 2\n",
        "# height = 360\n",
        "# fps = 24\n",
        "# quality = 75\n",
        "# format = \"webp\"       # webp | webm | gif\n",
        "# suffix = \"_reel\"\n",
        "\n",
        "[animated_sheet]\n",
        "# cols = 3             # ignored in auto mode\n",
        "# rows = 6             # ignored in auto mode\n",
        "# auto = false         # duration-driven grid; CLI --cols/--rows override this\n",
        "# width = 1280\n",
        "# gap = 8\n",
        "# clip_length_secs = 2\n",
        "# fps = 12\n",
        "# quality = 75\n",
        "# compression_effort = 4 # 0-6; 0 = lowest effort, not auto\n",
        "# thumb_font = 14\n",
        "# header_font = 18\n",
        "# show_timestamps = true\n",
        "# show_header = true\n",
        "# theme = \"dark\"\n",
        "# suffix = \"_animated_sheet\"\n",
        "\n",
        "[layout]\n",
        "# Shared duration-to-grid rules for sheet and animated-sheet --auto.\n",
        "# Tightest ceiling >= video duration wins, regardless of rule order.\n",
        "# Exactly one rule without max_seconds catches all longer videos.\n",
        "# Custom rules replace the whole ladder; error rule numbers are 1-based.\n",
        "# Animated auto grids are limited to 64 cells per rule, table-wide.\n",
        "# Still auto grids have no cell-count ceiling beyond 1..=32 per axis.\n",
        "# Each trim is capped at a quarter of the selected video's duration.\n",
        "# trim_start_secs = 5\n",
        "# trim_end_secs = 5\n",
        "# rules = [\n",
        "#   { max_seconds = 10,   cols = 3, rows = 1 },\n",
        "#   { max_seconds = 30,   cols = 3, rows = 2 },\n",
        "#   { max_seconds = 120,  cols = 3, rows = 3 },\n",
        "#   { max_seconds = 480,  cols = 3, rows = 4 },\n",
        "#   { max_seconds = 1200, cols = 3, rows = 5 },\n",
        "#   { max_seconds = 2100, cols = 3, rows = 6 },\n",
        "#   { max_seconds = 3000, cols = 3, rows = 7 },\n",
        "#   { max_seconds = 3600, cols = 3, rows = 8 },\n",
        "#   { cols = 4, rows = 10 },\n",
        "# ]\n",
    )
}

impl Config {
    pub fn layout_table(&self) -> LayoutTable {
        LayoutTable {
            trim_start_secs: self.layout.trim_start_secs.unwrap_or(defaults::layout::TRIM_START_SECS),
            trim_end_secs: self.layout.trim_end_secs.unwrap_or(defaults::layout::TRIM_END_SECS),
            rules: self.layout.rules.clone().unwrap_or_else(|| defaults::layout::RULES.to_vec()),
        }
    }

    /// Validate numeric fields against the same ranges enforced by the
    /// clap `value_parser!` declarations. CLI flags are already bounded
    /// at parse time; config values come through serde and need their own
    /// bounds check.
    pub fn validate(&self) -> Result<(), String> {
        check_range("screenshots.count", self.screenshots.count, 1, 1000)?;
        check_range("screenshots.quality", self.screenshots.quality, 50, 100)?;

        check_range("sheet.cols", self.sheet.cols, 1, 32)?;
        check_range("sheet.rows", self.sheet.rows, 1, 32)?;
        check_range("sheet.width", self.sheet.width, 320, 8192)?;
        check_range("sheet.gap", self.sheet.gap, 0, 200)?;
        check_range("sheet.thumb_font", self.sheet.thumb_font, 8, 72)?;
        check_range("sheet.header_font", self.sheet.header_font, 8, 72)?;
        check_range("sheet.quality", self.sheet.quality, 50, 100)?;

        check_range("reel.count", self.reel.count, 1, 100)?;
        check_range("reel.clip_length_secs", self.reel.clip_length_secs, 1, 60)?;
        check_range("reel.height", self.reel.height, 120, 4320)?;
        check_range("reel.fps", self.reel.fps, 1, 120)?;
        check_range("reel.quality", self.reel.quality, 0, 100)?;

        check_range("animated_sheet.cols", self.animated_sheet.cols, 1, 32)?;
        check_range("animated_sheet.rows", self.animated_sheet.rows, 1, 32)?;
        check_range("animated_sheet.width", self.animated_sheet.width, 320, 8192)?;
        check_range("animated_sheet.gap", self.animated_sheet.gap, 0, 200)?;
        check_range("animated_sheet.clip_length_secs", self.animated_sheet.clip_length_secs, 1, 60)?;
        check_range("animated_sheet.fps", self.animated_sheet.fps, 1, 120)?;
        check_range("animated_sheet.quality", self.animated_sheet.quality, 0, 100)?;
        check_range("animated_sheet.compression_effort", self.animated_sheet.compression_effort, 0, 6)?;
        check_range("animated_sheet.thumb_font", self.animated_sheet.thumb_font, 8, 72)?;
        check_range("animated_sheet.header_font", self.animated_sheet.header_font, 8, 72)?;
        if let Some((i, key)) = self.layout_rule_unknown.first() {
            return Err(format!("config: layout.rules[{i}] has unknown key '{key}'"));
        }
        auto_layout::validate_table(&self.layout_table())
    }
}

fn check_range(key: &str, value: Option<u32>, min: u32, max: u32) -> Result<(), String> {
    match value {
        Some(v) if v < min || v > max => {
            Err(format!("config: {key} = {v} out of range ({min}..={max})"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn layout_defaults_replacement_and_template_stay_in_sync() {
        let cfg = parse("[animated_sheet]\nauto = true\n", Path::new("config.toml")).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.animated_sheet.auto, Some(true));
        assert_eq!(cfg.layout_table().rules.len(), defaults::layout::RULES.len());
        let cfg = parse("[layout]\ntrim_start_secs = 0\nrules = [{cols=2,rows=1}]\n", Path::new("config.toml")).unwrap();
        cfg.validate().unwrap();
        let table = cfg.layout_table();
        assert_eq!((table.trim_start_secs, table.trim_end_secs, table.rules.len()), (0, 5, 1));
        let body = template().lines().map(|line| line.strip_prefix("# ").filter(|s| s.contains(" = ") || s.starts_with("  {") || *s == "]").unwrap_or(line)).collect::<Vec<_>>().join("\n");
        let cfg = parse(&body, Path::new("config.toml")).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.animated_sheet.auto, Some(false));
        assert_eq!(cfg.layout_table().rules.len(), defaults::layout::RULES.len());
        for (actual, expected) in cfg.layout_table().rules.iter().zip(defaults::layout::RULES) {
            assert_eq!((actual.max_seconds, actual.cols, actual.rows), (expected.max_seconds, expected.cols, expected.rows));
        }
    }

    #[test]
    fn raw_document_warnings_and_rule_key_scan_are_live() {
        let body = "[sheet]\nbogus = 99\n[animated_sheet]\nauto = true\n[unknown]\nx=1\n[layout]\ntrim_star_secs=5\nrules=[{cols=3,rows=2,max_second=30}]\n";
        let raw = toml::from_str::<toml::Table>(body).unwrap();
        assert_eq!(warn_unknown_keys(&raw, Path::new("config.toml")), vec![
            "warning: unknown key 'layout.trim_star_secs' in config.toml",
            "warning: unknown key 'sheet.bogus' in config.toml",
            "warning: unknown section 'unknown' in config.toml",
        ]);
        let cfg = parse(body, Path::new("config.toml")).unwrap();
        assert_eq!(cfg.validate().unwrap_err(), "config: layout.rules[1] has unknown key 'max_second'");
        let raw = toml::from_str::<toml::Table>("[animated_sheet]\nauto=true\n[layout]\ntrim_start_secs=5\n").unwrap();
        assert!(warn_unknown_keys(&raw, Path::new("config.toml")).is_empty());
    }

    #[test]
    fn existing_range_checks_still_precede_layout_errors() {
        let cfg = parse("[animated_sheet]\ncols=0\n[layout]\nrules=[{cols=1,rows=1,typo=1}]\n", Path::new("config.toml")).unwrap();
        assert_eq!(cfg.validate().unwrap_err(), "config: animated_sheet.cols = 0 out of range (1..=32)");
        for body in ["[layout]\nrules=[1]\n", "[layout]\ntrim_start_secs=-1\n", "[layout]\nrules=[{cols=2}]\n"] {
            assert!(parse(body, Path::new("config.toml")).is_err());
        }
    }

    #[test]
    fn compression_effort_template_matches_default() {
        assert!(template().contains(&format!("# compression_effort = {} # 0-6; 0 = lowest effort, not auto", acsm::defaults::animated_sheet::COMPRESSION_EFFORT)));
        let cfg = parse(template(), Path::new("config.toml")).unwrap();
        assert!(cfg.animated_sheet.compression_effort.is_none());
    }

    #[test]
    fn compression_effort_config_validates_every_supported_value() {
        for effort in 0..=6 {
            let cfg = parse(&format!("[animated_sheet]\ncompression_effort = {effort}\n"), Path::new("config.toml")).unwrap();
            assert_eq!(cfg.animated_sheet.compression_effort, Some(effort));
            assert!(cfg.validate().is_ok());
        }
    }

    #[test]
    fn load_missing_creates_template() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".acsm.toml");
        let cfg = load_or_create(&p, false).unwrap();
        assert!(p.exists(), "template should have been written");
        assert!(cfg.screenshots.count.is_none());
    }

    #[test]
    fn parse_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".acsm.toml");
        std::fs::write(&p, "[sheet]\ncols = 5\nrows = 4\n").unwrap();
        let cfg = load_or_create(&p, false).unwrap();
        assert_eq!(cfg.sheet.cols, Some(5));
        assert_eq!(cfg.sheet.rows, Some(4));
    }

    #[test]
    fn unknown_key_does_not_fail() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".acsm.toml");
        std::fs::write(&p, "[sheet]\ncols = 3\nbogus = 99\n").unwrap();
        let cfg = load_or_create(&p, false).unwrap(); // stderr warning is side-effect, not asserted here
        assert_eq!(cfg.sheet.cols, Some(3));
    }

    #[test]
    fn parse_failure_returns_err() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".acsm.toml");
        std::fs::write(&p, "[sheet]\ncols = \"not a number\"\n").unwrap();
        let err = load_or_create(&p, false).unwrap_err();
        assert!(matches!(err, ConfigError::ParseFailed { .. }));
    }

    #[test]
    fn validate_passes_on_empty_config() {
        let cfg = Config::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_cols() {
        let mut cfg = Config::default();
        cfg.sheet.cols = Some(0);
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("sheet.cols"), "got: {err}");
        assert!(err.contains("out of range"));
    }

    #[test]
    fn validate_rejects_zero_clip_length() {
        let mut cfg = Config::default();
        cfg.reel.clip_length_secs = Some(0);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_over_range_quality() {
        let mut cfg = Config::default();
        cfg.sheet.quality = Some(200);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn explicit_path_nonexistent_parent_does_not_crash() {
        // $ACSM_CONFIG pointing at a path whose parent dir doesn't exist.
        // Should return defaults, not error.
        let p = std::path::PathBuf::from("/nonexistent-parent-12345/cfg.toml");
        let cfg = load_or_create(&p, true).unwrap();
        assert!(cfg.screenshots.count.is_none());
    }
}
