//! System monitoring: once a minute LocalFlow notes CPU, memory, disk and battery
//! use. The history stays on this PC (in the LocalFlow database) and is kept for 30 days.
//!
//! Scripts read it with the `metrics` functions:
//!
//! ```lua
//! metrics.latest()                    -- the newest sample, or nil
//! metrics.recent(minutes)             -- samples from the last `minutes`, oldest first
//! metrics.average("cpu", minutes)     -- also "memory", "disk", "battery"
//! metrics.peak("cpu", minutes)        -- the highest value
//! metrics.lowest("battery", minutes)  -- the lowest value
//! ```

use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use mlua::{Lua, Table, Value};
use serde::Serialize;
use sqlx::SqlitePool;

/// How often a sample is taken.
pub const INTERVAL: Duration = Duration::from_secs(60);
/// Samples kept in memory for scripts: one week.
const KEEP_IN_MEMORY: usize = 7 * 24 * 60;
/// Samples kept in the database.
const KEEP_DAYS: i64 = 30;

/// One measurement. All values are percentages (0–100).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, sqlx::FromRow)]
pub struct Sample {
    /// Seconds since 1970.
    pub at: i64,
    pub cpu: f64,
    pub memory: f64,
    /// How full the Windows disk is.
    pub disk: f64,
    /// `None` on a PC without a battery.
    pub battery: Option<f64>,
}

impl Sample {
    pub fn value(&self, name: &str) -> Option<f64> {
        match name {
            "cpu" => Some(self.cpu),
            "memory" => Some(self.memory),
            "disk" => Some(self.disk),
            "battery" => self.battery,
            _ => None,
        }
    }
}

pub const NAMES: &[&str] = &["cpu", "memory", "disk", "battery"];

fn recent_samples() -> &'static Mutex<VecDeque<Sample>> {
    static RECENT: OnceLock<Mutex<VecDeque<Sample>>> = OnceLock::new();
    RECENT.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Add a sample to the in-memory history (oldest are dropped).
pub fn remember(sample: Sample) {
    let mut recent = recent_samples().lock().unwrap_or_else(|e| e.into_inner());
    recent.push_back(sample);
    while recent.len() > KEEP_IN_MEMORY {
        recent.pop_front();
    }
}

/// Samples newer than `minutes` ago, oldest first.
pub fn since(minutes: f64) -> Vec<Sample> {
    let from = chrono::Utc::now().timestamp() - (minutes * 60.0) as i64;
    let recent = recent_samples().lock().unwrap_or_else(|e| e.into_inner());
    recent.iter().filter(|s| s.at >= from).copied().collect()
}

pub fn latest() -> Option<Sample> {
    recent_samples().lock().unwrap_or_else(|e| e.into_inner()).back().copied()
}

/// Average, highest and lowest of one value, skipping samples without it.
pub fn summary(samples: &[Sample], name: &str) -> Option<(f64, f64, f64)> {
    let values: Vec<f64> = samples.iter().filter_map(|s| s.value(name)).collect();
    if values.is_empty() {
        return None;
    }
    let average = values.iter().sum::<f64>() / values.len() as f64;
    let peak = values.iter().copied().fold(f64::MIN, f64::max);
    let lowest = values.iter().copied().fold(f64::MAX, f64::min);
    Some((average, peak, lowest))
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Measure the PC now. Blocks for a moment (CPU use needs two readings).
pub fn measure() -> Sample {
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()).with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(250)));
    system.refresh_cpu_usage();
    let memory = if system.total_memory() > 0 {
        system.used_memory() as f64 / system.total_memory() as f64 * 100.0
    } else {
        0.0
    };

    // The disk Windows is on (or the root on other systems).
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "/".into()).to_lowercase();
    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .list()
        .iter()
        .find(|d| d.mount_point().to_string_lossy().to_lowercase().starts_with(&system_drive))
        .or_else(|| disks.list().first())
        .filter(|d| d.total_space() > 0)
        .map(|d| (d.total_space() - d.available_space()) as f64 / d.total_space() as f64 * 100.0)
        .unwrap_or(0.0);

    Sample {
        at: chrono::Utc::now().timestamp(),
        cpu: round(f64::from(system.global_cpu_usage())),
        memory: round(memory),
        disk: round(disk),
        battery: crate::lua::system::battery_status().map(|(percent, _, _)| f64::from(percent)),
    }
}

// ---- database -------------------------------------------------------------------

pub async fn save(pool: &SqlitePool, sample: &Sample) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metrics (at, cpu, memory, disk, battery) VALUES (?, ?, ?, ?, ?)")
        .bind(sample.at)
        .bind(sample.cpu)
        .bind(sample.memory)
        .bind(sample.disk)
        .bind(sample.battery)
        .execute(pool)
        .await?;
    let cutoff = sample.at - KEEP_DAYS * 24 * 60 * 60;
    sqlx::query("DELETE FROM metrics WHERE at < ?").bind(cutoff).execute(pool).await?;
    Ok(())
}

/// Samples from the database, newer than `minutes` ago, oldest first.
pub async fn load(pool: &SqlitePool, minutes: i64) -> sqlx::Result<Vec<Sample>> {
    let from = chrono::Utc::now().timestamp() - minutes * 60;
    sqlx::query_as("SELECT at, cpu, memory, disk, battery FROM metrics WHERE at >= ? ORDER BY at")
        .bind(from)
        .fetch_all(pool)
        .await
}

/// Fill the in-memory history from the database, then sample once a minute forever.
pub async fn run(pool: SqlitePool) {
    if let Ok(saved) = load(&pool, KEEP_IN_MEMORY as i64).await {
        for sample in saved {
            remember(sample);
        }
    }
    loop {
        if let Ok(sample) = tokio::task::spawn_blocking(measure).await {
            remember(sample);
            if let Err(e) = save(&pool, &sample).await {
                tracing::warn!("could not save a system sample: {e}");
            }
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

/// Group samples into at most `points` averages, for charts.
pub fn downsample(samples: &[Sample], points: usize) -> Vec<Sample> {
    if points == 0 || samples.len() <= points {
        return samples.to_vec();
    }
    let per = samples.len().div_ceil(points);
    samples
        .chunks(per)
        .map(|chunk| {
            let n = chunk.len() as f64;
            let batteries: Vec<f64> = chunk.iter().filter_map(|s| s.battery).collect();
            Sample {
                at: chunk[chunk.len() - 1].at,
                cpu: round(chunk.iter().map(|s| s.cpu).sum::<f64>() / n),
                memory: round(chunk.iter().map(|s| s.memory).sum::<f64>() / n),
                disk: round(chunk.iter().map(|s| s.disk).sum::<f64>() / n),
                battery: (!batteries.is_empty()).then(|| round(batteries.iter().sum::<f64>() / batteries.len() as f64)),
            }
        })
        .collect()
}

// ---- Lua ------------------------------------------------------------------------

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn sample_table(lua: &Lua, sample: &Sample) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("at", sample.at)?;
    table.set("cpu", sample.cpu)?;
    table.set("memory", sample.memory)?;
    table.set("disk", sample.disk)?;
    table.set("battery", sample.battery)?;
    Ok(table)
}

fn check_name(function: &str, name: &str) -> mlua::Result<()> {
    if NAMES.contains(&name) {
        Ok(())
    } else {
        Err(err(function, format!("unknown value \"{name}\"; use \"cpu\", \"memory\", \"disk\" or \"battery\"")))
    }
}

fn minutes(value: Option<f64>) -> f64 {
    value.unwrap_or(60.0).clamp(1.0, KEEP_IN_MEMORY as f64)
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    let metrics = lua.create_table()?;
    metrics.set(
        "latest",
        lua.create_function(|lua, ()| match latest() {
            Some(sample) => Ok(Value::Table(sample_table(lua, &sample)?)),
            None => Ok(Value::Nil),
        })?,
    )?;
    metrics.set(
        "recent",
        lua.create_function(|lua, m: Option<f64>| {
            let list = lua.create_table()?;
            for sample in since(minutes(m)) {
                list.push(sample_table(lua, &sample)?)?;
            }
            Ok(list)
        })?,
    )?;
    for (function, pick) in [("average", 0usize), ("peak", 1), ("lowest", 2)] {
        let full = format!("metrics.{function}");
        metrics.set(
            function,
            lua.create_function(move |_, (name, m): (String, Option<f64>)| {
                check_name(&full, &name)?;
                Ok(summary(&since(minutes(m)), &name).map(|(a, p, l)| round([a, p, l][pick])))
            })?,
        )?;
    }
    lua.globals().set("metrics", metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64, cpu: f64, battery: Option<f64>) -> Sample {
        Sample { at, cpu, memory: 50.0, disk: 40.0, battery }
    }

    #[test]
    fn summary_skips_missing_values() {
        let samples = [sample(1, 10.0, None), sample(2, 30.0, Some(80.0)), sample(3, 20.0, Some(60.0))];
        assert_eq!(summary(&samples, "cpu"), Some((20.0, 30.0, 10.0)));
        assert_eq!(summary(&samples, "battery"), Some((70.0, 80.0, 60.0)));
        assert_eq!(summary(&[sample(1, 5.0, None)], "battery"), None);
    }

    #[test]
    fn downsample_averages_groups() {
        let samples: Vec<_> = (0..10).map(|i| sample(i, i as f64, None)).collect();
        let small = downsample(&samples, 5);
        assert_eq!(small.len(), 5);
        assert_eq!(small[0].cpu, 0.5);
        assert_eq!(small[4].at, 9);
        assert_eq!(downsample(&samples, 50).len(), 10);
    }

    #[test]
    fn measuring_gives_sensible_percentages() {
        let s = measure();
        for value in [s.cpu, s.memory, s.disk] {
            assert!((0.0..=100.0).contains(&value), "{s:?}");
        }
    }
}
