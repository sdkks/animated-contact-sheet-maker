use acsm::auto_layout::AutoPlan;
use std::path::Path;

pub fn plan_line(path: &Path, plan: &AutoPlan) -> String {
    let prefix = format!("{}: plan · {:.2}s source · {}x{} grid · {} cells", path.display(), plan.duration_secs, plan.cols, plan.rows, plan.cells);
    match (plan.cell_length_secs, plan.decoded_secs) {
        (Some(length), Some(decoded)) => format!("{prefix} · {length:.2}s cells · {decoded:.2}s decoded"),
        _ => format!("{prefix} · {:.2}s span", plan.usable_secs),
    }
}
