// Terminal runner for the same probes the tray app runs. Usage: phase0 <data-dir>
fn main() {
    let data = std::env::args().nth(1).expect("usage: phase0 <data-dir>");
    let report = prbots_core::run_phase0(std::path::Path::new(&data));
    println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
}
