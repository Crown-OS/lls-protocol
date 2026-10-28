use std::time::Duration;

use anyhow::{Context as _, bail};
use llts_core::units::{Bitrate, FrameRate};

/// What the harness should measure.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BenchOptions {
    pub(crate) frames: u32,
    pub(crate) frame_rate: FrameRate,
    pub(crate) bitrate: Bitrate,
    /// One datagram in this many is discarded. Zero means a clean link.
    pub(crate) drop_every: usize,
    /// The gate: the run fails if p99 protocol latency exceeds this.
    pub(crate) budget: Duration,
    pub(crate) verbose: bool,
    /// Forces a fixed parity rate instead of letting the policy choose.
    pub(crate) forced_parity_percent: Option<u8>,
    /// Runs the screen share alone, without control messages and audio sharing the link.
    pub(crate) video_only: bool,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            frames: 600,
            frame_rate: FrameRate::STANDARD,
            bitrate: Bitrate::STANDARD,
            drop_every: 0,
            budget: Duration::from_millis(12),
            verbose: false,
            forced_parity_percent: None,
            video_only: false,
        }
    }
}

impl BenchOptions {
    pub(crate) const USAGE: &'static str = "\
usage: llts-bench [options]

  --frames N        frames to send            (default 600)
  --fps N           frame rate in hertz       (default 60)
  --kbps N          media bitrate             (default 12000)
  --drop-every N    discard one datagram in N (default 0, a clean link)
  --budget-ms N     p99 gate in milliseconds  (default 12)
  --fec-percent N   force a fixed parity rate instead of adapting
  --video-only      no control messages or audio alongside the video
  --verbose         print every stage
  --help
";

    /// Bytes one frame occupies at the configured rate.
    pub(crate) fn frame_bytes(&self) -> usize {
        usize::try_from(self.bitrate.bytes_in(self.frame_rate.period())).unwrap_or(0)
    }

    pub(crate) fn parse(arguments: impl Iterator<Item = String>) -> anyhow::Result<Option<Self>> {
        let mut options = Self::default();
        let mut arguments = arguments.peekable();

        while let Some(flag) = arguments.next() {
            let mut value = || -> anyhow::Result<String> {
                arguments
                    .next()
                    .with_context(|| format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "--help" | "-h" => return Ok(None),
                "--verbose" | "-v" => options.verbose = true,
                "--video-only" => options.video_only = true,
                "--frames" => options.frames = value()?.parse().context("--frames")?,
                "--fps" => {
                    let hertz: u16 = value()?.parse().context("--fps")?;
                    let hertz =
                        std::num::NonZeroU16::new(hertz).context("--fps must be above 0")?;
                    options.frame_rate = FrameRate::from_hz(hertz);
                }
                "--kbps" => {
                    let kbps: u32 = value()?.parse().context("--kbps")?;
                    options.bitrate = Bitrate::from_kilobits_per_second(kbps)
                        .context("--kbps must be above 0")?;
                }
                "--drop-every" => options.drop_every = value()?.parse().context("--drop-every")?,
                "--fec-percent" => {
                    options.forced_parity_percent =
                        Some(value()?.parse().context("--fec-percent")?);
                }
                "--budget-ms" => {
                    let millis: u64 = value()?.parse().context("--budget-ms")?;
                    options.budget = Duration::from_millis(millis);
                }
                other => bail!("unrecognised option {other}\n\n{}", Self::USAGE),
            }
        }
        Ok(Some(options))
    }
}
