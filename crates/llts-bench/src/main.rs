//! Latency harness for the LLTS media link.
//!
//! Runs two protocol engines over real loopback sockets with a synthetic encoder, and reports the
//! percentile the CI gate reads. A budget you cannot attribute is a budget you cannot optimise,
//! so this exists before the pipeline it measures.

mod loopback;
mod options;
mod report;

use std::process::ExitCode;

use anyhow::Context as _;

use crate::loopback::Loopback;
use crate::options::BenchOptions;

fn main() -> anyhow::Result<ExitCode> {
    let Some(options) = BenchOptions::parse(std::env::args().skip(1))? else {
        print!("{}", BenchOptions::USAGE);
        return Ok(ExitCode::SUCCESS);
    };

    let report = Loopback::new(options)
        .context("preparing the loopback")?
        .run()
        .context("running the loopback")?;
    report.print();

    Ok(if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
