//! Duration-driven grids and nominal source geometry, independent of media I/O.
use crate::layout::checked_sheet_layout;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const RULE_KEYS: &[&str] = &["max_seconds", "cols", "rows"];
pub const MIN_AUTO_CELLS: u32 = 2;
/// Bounds animated stitch memory (one decoder per cell), not still-sheet grids.
pub const MAX_AUTO_CELLS: u32 = 64;

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutRule {
    pub max_seconds: Option<u32>,
    pub cols: u32,
    pub rows: u32,
}

#[derive(Debug, Clone)]
pub struct LayoutTable {
    pub trim_start_secs: u32,
    pub trim_end_secs: u32,
    pub rules: Vec<LayoutRule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutoPlan {
    pub cols: u32,
    pub rows: u32,
    pub cells: u32,
    pub duration_secs: f64,
    pub trim_start_secs: f64,
    pub trim_end_secs: f64,
    pub usable_secs: f64,
    pub segment_secs: f64,
    pub cell_length_secs: Option<f64>,
    pub decoded_secs: Option<f64>,
    pub clip_starts: Vec<f64>,
    pub centres: Vec<f64>,
}

pub fn validate_table(table: &LayoutTable) -> Result<(), String> {
    if table.rules.is_empty() {
        return Err("config: layout.rules must not be empty".into());
    }
    let catch_alls = table.rules.iter().filter(|r| r.max_seconds.is_none()).count();
    if catch_alls != 1 {
        let message = "config: layout.rules must contain exactly one rule without max_seconds (the catch-all for videos longer than every ceiling)";
        return Err(if catch_alls == 0 { message.into() } else { format!("{message}; found {catch_alls}") });
    }
    let mut ceilings = BTreeMap::new();
    for (i, rule) in table.rules.iter().enumerate() {
        if let Some(max) = rule.max_seconds
            && let Some(first) = ceilings.insert(max, i + 1) {
            return Err(format!("config: layout.rules has a duplicate max_seconds = {max} (rules {first} and {})", i + 1));
        }
    }
    // Run each error class over the whole table before moving to the next.
    for (i, rule) in table.rules.iter().enumerate() {
        for (key, value) in [("cols", rule.cols), ("rows", rule.rows)] {
            if !(1..=32).contains(&value) {
                return Err(format!("config: layout.rules[{}].{key} = {value} out of range (1..=32)", i + 1));
            }
        }
        if rule.max_seconds == Some(0) {
            return Err(format!("config: layout.rules[{}].max_seconds = 0 out of range (1..)", i + 1));
        }
    }
    for (i, rule) in table.rules.iter().enumerate() {
        let cells = rule.cols * rule.rows;
        if cells < MIN_AUTO_CELLS {
            return Err(format!("config: layout.rules[{}] must produce at least 2 cells (got {cells} from {}x{})", i + 1, rule.cols, rule.rows));
        }
    }
    Ok(())
}

/// Command-scoped, table-wide check before any animated auto job starts.
pub fn validate_animated_table(table: &LayoutTable) -> Result<(), String> {
    for (i, rule) in table.rules.iter().enumerate() {
        let cells = u64::from(rule.cols) * u64::from(rule.rows);
        if cells > u64::from(MAX_AUTO_CELLS) {
            return Err(format!("config: layout.rules[{}] produces {cells} cells ({}x{}), over the {MAX_AUTO_CELLS}-cell animated auto-layout limit", i + 1, rule.cols, rule.rows));
        }
    }
    Ok(())
}

fn match_rule(rules: &[LayoutRule], duration: f64) -> Option<&LayoutRule> {
    rules.iter().filter(|r| r.max_seconds.is_some_and(|max| f64::from(max) >= duration))
        .min_by_key(|r| r.max_seconds)
        .or_else(|| rules.iter().find(|r| r.max_seconds.is_none()))
}

pub fn resolve(
    table: &LayoutTable,
    duration_secs: f64,
    width: u32,
    gap: u32,
    clip_length_secs: Option<f64>,
) -> Result<AutoPlan, String> {
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Err(format!("cannot lay out a {duration_secs:.2}s video (the probed duration is not usable)"));
    }
    if clip_length_secs.is_some_and(|l| !l.is_finite() || l <= 0.0) {
        return Err("auto clip length must be positive and finite".into());
    }
    let rule = match_rule(&table.rules, duration_secs)
        .ok_or_else(|| format!("no rule covers a {duration_secs:.2}s video"))?;
    if !(1..=32).contains(&rule.cols) || !(1..=32).contains(&rule.rows) {
        return Err("auto grid axes must be in 1..=32".into());
    }
    let cells = rule.cols * rule.rows;
    if cells < MIN_AUTO_CELLS { return Err("auto grid produces fewer than 2 cells".into()); }
    checked_sheet_layout(rule.cols, rule.rows, width, gap)?;
    let trim_start_secs = f64::from(table.trim_start_secs).min(duration_secs / 4.0);
    let trim_end_secs = f64::from(table.trim_end_secs).min(duration_secs / 4.0);
    let usable_secs = duration_secs - trim_start_secs - trim_end_secs;
    let segment_secs = usable_secs / f64::from(cells);
    let cell_length_secs = clip_length_secs.map(|l| l.min(segment_secs));
    let centres = (0..cells).map(|i| trim_start_secs + (f64::from(i) + 0.5) * segment_secs).collect();
    let clip_starts = cell_length_secs.map(|l| (0..cells)
        .map(|i| trim_start_secs + f64::from(i) * segment_secs + (segment_secs - l) / 2.0)
        .collect()).unwrap_or_default();
    // These are nominal spans. The existing seek helper rounds to milliseconds;
    // ffmpeg quantises fractional durations to the output frame grid.
    Ok(AutoPlan {
        cols: rule.cols, rows: rule.rows, cells, duration_secs, trim_start_secs,
        trim_end_secs, usable_secs, segment_secs, cell_length_secs,
        decoded_secs: cell_length_secs.map(|l| f64::from(cells) * l), clip_starts, centres,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn defaults() -> LayoutTable {
        LayoutTable { trim_start_secs: 5, trim_end_secs: 5, rules: crate::defaults::layout::RULES.to_vec() }
    }
    #[test]
    fn ceilings_and_geometry_are_order_independent() {
        let table = defaults();
        validate_table(&table).unwrap();
        validate_animated_table(&table).unwrap();
        let mut shuffled = table.clone();
        shuffled.rules.reverse();
        for (d, rows) in [(1.,1), (3.,1), (10.,1), (30.,2), (45.,3), (120.,3), (480.,4), (1200.,5), (2100.,6), (3000.,7), (3600.,8), (7200.,10)] {
            let plan = resolve(&table, d, 1920, 8, Some(5.)).unwrap();
            assert_eq!(plan.rows, rows);
            assert_eq!(plan, resolve(&shuffled, d, 1920, 8, Some(5.)).unwrap());
        }
    }
    #[test]
    fn nominal_spans_tile_or_sample_without_overlap() {
        for duration in [0.001, 1., 3., 10., 45., 120., 3600., 7200.] {
            for length in [0.01, 1., 2., 5., 60.] {
                let p = resolve(&defaults(), duration, 1920, 8, Some(length)).unwrap();
                let l = p.cell_length_secs.unwrap();
                assert!(l <= length);
                assert!(p.decoded_secs.unwrap() <= p.usable_secs + 1e-9);
                assert!(p.trim_start_secs <= duration / 4.);
                assert!(p.trim_end_secs <= duration / 4.);
                for (i, &start) in p.clip_starts.iter().enumerate() {
                    assert!(start >= p.trim_start_secs - 1e-9);
                    assert!(start + l <= duration - p.trim_end_secs + 1e-9);
                    assert!((start + l / 2. - p.centres[i]).abs() < 1e-9);
                    if i > 0 { assert!(p.clip_starts[i - 1] + l <= start + 1e-9); }
                    if p.segment_secs <= length {
                        assert!((start - (p.trim_start_secs + i as f64 * l)).abs() < 1e-9);
                    }
                }
            }
        }
        let p = resolve(&defaults(), 45., 1280, 8, Some(5.)).unwrap();
        assert_eq!((p.cols, p.rows, p.cells), (3, 3, 9));
        assert_eq!((p.trim_start_secs, p.trim_end_secs, p.usable_secs), (5., 5., 35.));
        assert!((p.cell_length_secs.unwrap() - 35. / 9.).abs() < 1e-12);
        let p = resolve(&defaults(), 3600., 1280, 8, Some(2.)).unwrap();
        assert_eq!(p.decoded_secs, Some(48.));
    }
    #[test]
    fn rejects_unusable_duration_and_clip_length() {
        for d in [0., -1., f64::NAN, f64::INFINITY] { assert!(resolve(&defaults(), d, 1280, 8, Some(5.)).is_err()); }
        for l in [0., -1., f64::NAN, f64::INFINITY] { assert!(resolve(&defaults(), 45., 1280, 8, Some(l)).is_err()); }
        assert!(resolve(&defaults(), 45., 320, 200, None).unwrap_err().contains("too narrow"));
        let mut t = defaults(); t.rules.retain(|r| r.max_seconds.is_some());
        assert_eq!(resolve(&t, 7200., 1280, 8, None).unwrap_err(), "no rule covers a 7200.00s video");
    }
    #[test]
    fn validation_messages_and_command_scoped_ceiling() {
        let cases = [
            (vec![], "config: layout.rules must not be empty"),
            (vec![(Some(30),3,2)], "config: layout.rules must contain exactly one rule without max_seconds (the catch-all for videos longer than every ceiling)"),
            (vec![(None,3,2),(None,3,3)], "config: layout.rules must contain exactly one rule without max_seconds (the catch-all for videos longer than every ceiling); found 2"),
            (vec![(None,3,2),(Some(30),3,2),(Some(30),3,3)], "config: layout.rules has a duplicate max_seconds = 30 (rules 2 and 3)"),
            (vec![(None,3,2),(Some(30),0,2)], "config: layout.rules[2].cols = 0 out of range (1..=32)"),
            (vec![(None,3,2),(Some(30),3,33)], "config: layout.rules[2].rows = 33 out of range (1..=32)"),
            (vec![(None,3,2),(Some(0),3,2)], "config: layout.rules[2].max_seconds = 0 out of range (1..)"),
            (vec![(None,3,2),(Some(30),1,1)], "config: layout.rules[2] must produce at least 2 cells (got 1 from 1x1)"),
        ];
        for (rules, expected) in cases {
            let t = LayoutTable { rules: rules.into_iter().map(|(max_seconds,cols,rows)| LayoutRule {max_seconds,cols,rows}).collect(), ..defaults() };
            assert_eq!(validate_table(&t).unwrap_err(), expected);
            let mut reversed = t.clone(); reversed.rules.reverse();
            assert!(validate_table(&reversed).is_err());
        }
        let mut t = defaults();
        t.rules = vec![LayoutRule {max_seconds: Some(100_000), cols: 4, rows: 10}, LayoutRule {max_seconds: None, cols: 16, rows: 8}];
        validate_table(&t).unwrap();
        assert_eq!(validate_animated_table(&t).unwrap_err(), "config: layout.rules[2] produces 128 cells (16x8), over the 64-cell animated auto-layout limit");
        assert_eq!(resolve(&t, 200_000., 1920, 8, None).unwrap().cells, 128);
    }
}
