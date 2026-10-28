use std::time::Duration;

use llts_core::stats::{LatencyHistogram, StatsSnapshot};

use crate::options::BenchOptions;

/// Latency and delivery for one kind of traffic sharing the link with the video.
#[derive(Debug, Default)]
pub(crate) struct SideTraffic {
    pub(crate) sent: u64,
    pub(crate) delivered: u64,
    latency: LatencyHistogram,
}

impl SideTraffic {
    pub(crate) fn record(&mut self, latency: Duration) {
        self.delivered += 1;
        self.latency.record(latency);
    }

    fn p99(&self) -> Duration {
        self.latency.quantile(99)
    }
}

/// What one run measured.
#[derive(Debug)]
pub(crate) struct BenchReport {
    pub(crate) options: BenchOptions,
    pub(crate) frames_sent: u32,
    pub(crate) datagrams_sent: u64,
    pub(crate) datagrams_dropped: u64,
    pub(crate) bytes_sent: u64,
    pub(crate) wall_clock: Duration,
    pub(crate) latency_p50: Duration,
    pub(crate) latency_p95: Duration,
    pub(crate) latency_p99: Duration,
    pub(crate) latency_worst: Duration,
    pub(crate) audio: SideTraffic,
    pub(crate) control: SideTraffic,
    pub(crate) receiver: StatsSnapshot,
    pub(crate) sender: StatsSnapshot,
}

impl BenchReport {
    /// Whether the run stayed inside its p99 budget with every frame and every control message
    /// delivered, and audio inside the same budget.
    pub(crate) fn passed(&self) -> bool {
        self.latency_p99 <= self.options.budget
            && self.receiver.frames_presented >= u64::from(self.frames_sent)
            && self.control.delivered >= self.control.sent
            && self.audio.p99() <= self.options.budget
    }

    pub(crate) fn print(&self) {
        let link = if self.options.drop_every == 0 {
            "clean".to_owned()
        } else {
            format!("1 in {} dropped", self.options.drop_every)
        };

        let mux = if self.options.video_only {
            "video only"
        } else {
            "video + audio + control"
        };
        println!("llts-bench  loopback  {link}  {mux}");
        println!(
            "  {} frames at {} Hz, {} kbit/s, {} bytes per frame",
            self.frames_sent,
            self.options.frame_rate.hz(),
            self.options.bitrate.bits_per_second() / 1_000,
            self.options.frame_bytes(),
        );
        println!(
            "  {} datagrams, {} discarded, {:.1} MB over {:.2} s",
            self.datagrams_sent,
            self.datagrams_dropped,
            self.bytes_sent as f64 / 1_048_576.0,
            self.wall_clock.as_secs_f64(),
        );
        println!();
        println!("  protocol latency   encode submit to frame presented");
        println!("    p50   {}", millis(self.latency_p50));
        println!("    p95   {}", millis(self.latency_p95));
        println!("    p99   {}", millis(self.latency_p99));
        println!("    max   {}", millis(self.latency_worst));
        println!();
        if !self.options.video_only {
            println!("  sharing the link");
            println!(
                "    audio        {} of {} frames, p99 {}",
                self.audio.delivered,
                self.audio.sent,
                millis(self.audio.p99())
            );
            println!(
                "    control      {} of {} messages, p99 {}",
                self.control.delivered,
                self.control.sent,
                millis(self.control.p99())
            );
            println!();
        }
        println!("  delivery");
        println!("    presented    {}", self.receiver.frames_presented);
        println!("    superseded   {}", self.receiver.frames_superseded);
        println!("    expired      {}", self.receiver.frames_expired);
        println!("    incomplete   {}", self.receiver.frames_incomplete);
        println!(
            "    repaired     {} fragments",
            self.receiver.fragments_repaired
        );
        println!(
            "    retransmits  {} requested, {} abandoned",
            self.receiver.retransmissions_requested, self.receiver.retransmissions_abandoned
        );
        println!("    playout hold {}", millis(self.receiver.playout_delay));
        println!();
        println!("  what the sender believes about the link");
        println!(
            "    loss         {:.2} %",
            f64::from(self.sender.loss_rate.proportion().get()) / 10.0
        );
        println!("    round trip   {}", millis(self.sender.round_trip));
        println!(
            "    pacing       {} kbit/s",
            self.sender.pacing_rate.bits_per_second() / 1_000
        );
        println!();

        let verdict = if self.passed() { "PASS" } else { "FAIL" };
        println!(
            "  {verdict}  p99 {} against a {} budget",
            millis(self.latency_p99),
            millis(self.options.budget),
        );
    }
}

fn millis(span: Duration) -> String {
    format!("{:>7.2} ms", span.as_secs_f64() * 1_000.0)
}
