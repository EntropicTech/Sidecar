use crate::model::{Availability, Metric};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Client {
    pub pci: String,
    pub id: Option<u64>,
    pub engines: BTreeMap<String, u64>,
    pub capacities: BTreeMap<String, u64>,
    pub memory: BTreeMap<String, u64>,
}

pub fn parse(text: &str) -> Option<Client> {
    let mut result = Client::default();
    let mut driver = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "drm-driver" => driver = value == "amdgpu",
            "drm-pdev" => result.pci = value.to_lowercase(),
            "drm-client-id" => result.id = value.parse().ok(),
            _ => {
                if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
                    if let Ok(n) = value.parse::<u64>() {
                        result.capacities.insert(engine.into(), n.max(1));
                    }
                } else if let Some(engine) = key.strip_prefix("drm-engine-") {
                    if let Some(n) = value.strip_suffix(" ns").and_then(|s| s.parse().ok()) {
                        result.engines.insert(engine.into(), n);
                    }
                } else if let Some(name) = key.strip_prefix("drm-") {
                    if [
                        "resident-",
                        "total-",
                        "shared-",
                        "active-",
                        "purgeable-",
                        "memory-",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                    {
                        if let Some(n) = memory_bytes(value) {
                            result.memory.insert(name.into(), n);
                        }
                    }
                }
            }
        }
    }
    // AMD's legacy memory field aliases resident memory; never account for both.
    let aliases: Vec<_> = result
        .memory
        .keys()
        .filter_map(|k| k.strip_prefix("memory-").map(str::to_owned))
        .collect();
    for region in aliases {
        if let Some(value) = result.memory.remove(&format!("memory-{region}")) {
            result
                .memory
                .entry(format!("resident-{region}"))
                .or_insert(value);
        }
    }
    (driver && !result.pci.is_empty()).then_some(result)
}

fn memory_bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    let multiplier = match parts.next() {
        None | Some("B") => 1,
        Some("KiB") => 1024,
        Some("MiB") => 1024 * 1024,
        _ => return None,
    };
    n.checked_mul(multiplier)
}

#[derive(Default)]
pub struct EngineCounters {
    previous: BTreeMap<(String, u64, String), (u64, u64)>,
}

impl EngineCounters {
    pub fn sample(&mut self, client: &Client, now: u64) -> BTreeMap<String, Metric> {
        let Some(id) = client.id else {
            return BTreeMap::new();
        };
        client
            .engines
            .iter()
            .map(|(engine, &counter)| {
                let key = (client.pci.clone(), id, engine.clone());
                let source = format!("proc/fdinfo:{}:{id}:drm-engine-{engine}", client.pci);
                let metric = match self.previous.get(&key).copied() {
                    Some((old, time))
                        if counter >= old && now > time && now - time <= 5_000_000_000 =>
                    {
                        let capacity = *client.capacities.get(engine).unwrap_or(&1) as f64;
                        Metric::measured(
                            ((counter - old) as f64 / (now - time) as f64 * 100.0 / capacity)
                                .min(100.0),
                            "% engine time",
                            source,
                        )
                    }
                    Some((old, _)) if counter < old => {
                        Metric::missing("% engine time", source, Availability::Stale)
                    }
                    _ => Metric::missing("% engine time", source, Availability::WarmingUp),
                };
                // Retain the high water value AND its timestamp through regressions.
                if self
                    .previous
                    .get(&key)
                    .is_none_or(|(old, _)| counter >= *old)
                {
                    self.previous.insert(key, (counter, now));
                }
                (engine.clone(), metric)
            })
            .collect()
    }
    pub fn retain_clients(&mut self, live: &std::collections::BTreeSet<(String, u64)>) {
        self.previous
            .retain(|(pci, id, _), _| live.contains(&(pci.clone(), *id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_units_unknown_fields_and_aliases() {
        let c = parse("drm-driver: amdgpu\ndrm-pdev: 0000:01:00.0\ndrm-client-id: 7\ndrm-engine-gfx: 100 ns\ndrm-engine-capacity-gfx: 2\ndrm-memory-vram: 2 KiB\ndrm-resident-vram: 3 KiB\nfuture: anything").unwrap();
        assert_eq!(c.memory.len(), 1);
        assert_eq!(c.memory["resident-vram"], 3072);
        assert_eq!(c.capacities["gfx"], 2);
        assert!(parse("drm-driver: i915\ndrm-pdev: 0000:00:02.0").is_none());
    }
    #[test]
    fn regression_holds_high_water_and_recovers() {
        let mut counters = EngineCounters::default();
        let mut c = Client {
            pci: "gpu".into(),
            id: Some(1),
            ..Default::default()
        };
        c.engines.insert("gfx".into(), 100);
        assert_eq!(
            counters.sample(&c, 1000)["gfx"].status,
            Availability::WarmingUp
        );
        c.engines.insert("gfx".into(), 50);
        assert_eq!(counters.sample(&c, 2000)["gfx"].status, Availability::Stale);
        c.engines.insert("gfx".into(), 300);
        assert_eq!(counters.sample(&c, 3000)["gfx"].value, Some(10.0));
        assert_eq!(
            counters.sample(&c, 9_000_000_000)["gfx"].status,
            Availability::WarmingUp
        );
    }
}
