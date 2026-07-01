//! Real system statistics sampled from Linux `/proc`, no external crates.
//!
//! CPU and network are rates, so they're computed from deltas between samples.
//! [`SysStats::update`] is cheap and self-throttles to ~1s, so it can be called
//! every frame.

use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Default, Clone, Copy)]
pub struct SysStats {
    pub cpu_pct: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub mem_pct: f32,
    /// Receive / transmit throughput in bytes per second.
    pub rx_rate: f64,
    pub tx_rate: f64,
    pub load1: f32,

    // Internal sampling state.
    last_sample: Option<Instant>,
    prev_cpu: Option<(u64, u64)>, // (idle, total)
    prev_net: Option<(u64, u64)>, // (rx, tx)
}

impl SysStats {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.sample(0.0);
        s
    }

    /// Refresh the stats if at least `SAMPLE_INTERVAL` has elapsed.
    pub fn update(&mut self) {
        let now = Instant::now();
        let dt = match self.last_sample {
            Some(t) => now.duration_since(t),
            None => SAMPLE_INTERVAL,
        };
        if dt >= SAMPLE_INTERVAL {
            self.sample(dt.as_secs_f64());
        }
    }

    fn sample(&mut self, dt: f64) {
        self.last_sample = Some(Instant::now());

        if let Some((idle, total)) = read_cpu() {
            if let Some((pidle, ptotal)) = self.prev_cpu {
                let d_idle = idle.saturating_sub(pidle) as f64;
                let d_total = total.saturating_sub(ptotal) as f64;
                if d_total > 0.0 {
                    self.cpu_pct = ((1.0 - d_idle / d_total) * 100.0).clamp(0.0, 100.0) as f32;
                }
            }
            self.prev_cpu = Some((idle, total));
        }

        if let Some((total, avail)) = read_mem() {
            self.mem_total = total;
            self.mem_used = total.saturating_sub(avail);
            self.mem_pct = if total > 0 {
                (self.mem_used as f32 / total as f32) * 100.0
            } else {
                0.0
            };
        }

        if let Some((rx, tx)) = read_net() {
            if let Some((prx, ptx)) = self.prev_net.filter(|_| dt > 0.0) {
                self.rx_rate = rx.saturating_sub(prx) as f64 / dt;
                self.tx_rate = tx.saturating_sub(ptx) as f64 / dt;
            }
            self.prev_net = Some((rx, tx));
        }

        if let Some(load) = read_load() {
            self.load1 = load;
        }
    }
}

/// Aggregate CPU time from `/proc/stat`: returns (idle, total) jiffies.
fn read_cpu() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?; // "cpu  u n s idle iowait irq ..."
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.filter_map(|v| v.parse().ok()).collect();
    if values.len() < 5 {
        return None;
    }
    let idle = values[3] + values[4]; // idle + iowait
    let total: u64 = values.iter().sum();
    Some((idle, total))
}

/// Total and available memory in bytes from `/proc/meminfo`.
fn read_mem() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut avail = None;
    for line in info.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_kb(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail = parse_kb(rest);
        }
        if total.is_some() && avail.is_some() {
            break;
        }
    }
    Some((total?, avail?))
}

fn parse_kb(s: &str) -> Option<u64> {
    s.split_whitespace().next()?.parse::<u64>().ok().map(|kb| kb * 1024)
}

/// Total received / transmitted bytes across all real interfaces (skips `lo`).
fn read_net() -> Option<(u64, u64)> {
    let dev = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in dev.lines() {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface.trim();
        if iface == "lo" {
            continue;
        }
        let cols: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        // rx bytes = col 0, tx bytes = col 8.
        if cols.len() >= 9 {
            rx += cols[0];
            tx += cols[8];
        }
    }
    Some((rx, tx))
}

fn read_load() -> Option<f32> {
    let load = std::fs::read_to_string("/proc/loadavg").ok()?;
    load.split_whitespace().next()?.parse().ok()
}

/// Human-readable size, e.g. `3.9G`, `512M`.
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{v:.0}{}", UNITS[unit])
    } else {
        format!("{v:.1}{}", UNITS[unit])
    }
}

/// Human-readable throughput, e.g. `1.2M/s`.
pub fn fmt_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", fmt_size(bytes_per_sec.max(0.0) as u64))
}
