//! Optional local display labels. PCI identity and raw diagnostic names stay intact.
use crate::fsutil::read_text_limited;
use std::{collections::BTreeMap, path::Path};

#[derive(Default)]
pub(crate) struct Database {
    products: BTreeMap<(u16, u8), Option<String>>,
    cpu: Option<String>,
}

impl Database {
    pub(crate) fn load(ids: &Path, cpuinfo: &Path) -> Self {
        Self {
            products: read_text_limited(ids, 512 * 1024)
                .map(|text| parse_products(&text))
                .unwrap_or_default(),
            cpu: read_text_limited(cpuinfo, 1024 * 1024)
                .ok()
                .and_then(|text| cpu_model(&text)),
        }
    }

    pub(crate) fn label(&self, device: &Path, integrated: bool) -> Option<(String, String)> {
        if let Some(name) = read_text_limited(&device.join("product_name"), 256)
            .ok()
            .and_then(|text| clean_name(&text))
        {
            return Some((name, "sysfs product_name".into()));
        }
        let id = read_text_limited(&device.join("device"), 32)
            .ok()
            .and_then(|text| hex(&text))
            .and_then(|n| u16::try_from(n).ok());
        let revision = read_text_limited(&device.join("revision"), 32)
            .ok()
            .and_then(|text| hex(&text))
            .and_then(|n| u8::try_from(n).ok());
        if let Some(name) = id
            .zip(revision)
            .and_then(|key| self.products.get(&key))
            .and_then(Option::as_ref)
        {
            return Some((name.clone(), "libdrm amdgpu.ids (device + revision)".into()));
        }
        if integrated {
            return self.cpu.as_ref().map(|cpu| {
                (
                    format!("{} graphics", cpu.strip_prefix("AMD ").unwrap_or(cpu)),
                    "Host CPU model; GPU model unavailable".into(),
                )
            });
        }
        None
    }
}

fn clean_name(text: &str) -> Option<String> {
    let name = text.trim();
    (!name.is_empty() && name.len() <= 160 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}
fn hex(text: &str) -> Option<u32> {
    let value = text.trim();
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    (!value.is_empty() && value.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}
fn parse_products(text: &str) -> BTreeMap<(u16, u8), Option<String>> {
    let mut products: BTreeMap<(u16, u8), Option<String>> = BTreeMap::new();
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        let fields: Vec<_> = line.splitn(3, ',').collect();
        if fields.len() != 3 {
            continue;
        }
        let Some(id) = hex(fields[0]).and_then(|n| u16::try_from(n).ok()) else {
            continue;
        };
        let Some(revision) = hex(fields[1]).and_then(|n| u8::try_from(n).ok()) else {
            continue;
        };
        let Some(name) = clean_name(fields[2]) else {
            continue;
        };
        products
            .entry((id, revision))
            .and_modify(|existing| {
                // Conflicting rows cannot confidently identify a product.
                if existing.as_ref() != Some(&name) {
                    *existing = None;
                }
            })
            .or_insert(Some(name));
    }
    products
}
fn cpu_model(text: &str) -> Option<String> {
    let models: std::collections::BTreeSet<_> = text
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name")
                .then(|| clean_name(value))
                .flatten()
        })
        .collect();
    if models.len() != 1 {
        return None;
    }
    let model = models.into_iter().next()?;
    // Avoid pretending a mixed/unknown processor inventory names the GPU.
    if !model.starts_with("AMD ") {
        return None;
    }
    let model = model.strip_suffix(" Processor").unwrap_or(&model);
    let model = model
        .rsplit_once(' ')
        .filter(|(_, last)| {
            last.strip_suffix("-Core")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|(base, _)| base)
        .unwrap_or(model);
    clean_name(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_revision_names_do_not_guess_between_shared_device_ids() {
        let db = parse_products("# comment\n1.0.0\n744C, C8, Radeon RX 7900 XTX\n744c, cc, Radeon RX 7900 XT\n744C, CE, Radeon RX 7900 GRE\nbroken\n744c,zz,bad\n10000,CC,overflow\n744c,1ff,overflow\n744c,ff,\n");
        assert_eq!(db[&(0x744c, 0xcc)].as_deref(), Some("Radeon RX 7900 XT"));
        assert_eq!(db.len(), 3);
        assert!(!db.contains_key(&(0x744c, 0xcb)));
        let duplicate = parse_products("744c,cc,One\n744c,cc,Two\n744c,cc,One");
        assert_eq!(duplicate[&(0x744c, 0xcc)], None);
    }
    #[test]
    fn local_labels_prioritize_kernel_then_exact_database_then_cpu_context() {
        let dir = tempfile::tempdir().unwrap();
        let device = dir.path().join("device");
        std::fs::create_dir(&device).unwrap();
        let ids = dir.path().join("amdgpu.ids");
        let cpu = dir.path().join("cpuinfo");
        std::fs::write(&ids, "744C, CC, AMD Radeon RX 7900 XT").unwrap();
        std::fs::write(&cpu, "model name : AMD Ryzen 7 9800X3D 8-Core Processor\nmodel name : AMD Ryzen 7 9800X3D 8-Core Processor\n").unwrap();
        std::fs::write(device.join("device"), "0x744c\n").unwrap();
        std::fs::write(device.join("revision"), "0xcc\n").unwrap();
        let db = Database::load(&ids, &cpu);
        assert_eq!(db.label(&device, false).unwrap().0, "AMD Radeon RX 7900 XT");
        std::fs::write(device.join("revision"), "0xcb").unwrap();
        assert!(db.label(&device, false).is_none());
        assert_eq!(
            db.label(&device, true).unwrap().0,
            "Ryzen 7 9800X3D graphics"
        );
        std::fs::write(device.join("product_name"), "Kernel GPU\n").unwrap();
        assert_eq!(db.label(&device, true).unwrap().0, "Kernel GPU");
        std::fs::write(device.join("product_name"), "\n").unwrap();
        assert!(db.label(&device, false).is_none());
        assert!(
            Database::load(&dir.path().join("missing"), &dir.path().join("absent"))
                .label(&device, true)
                .is_none()
        );
        std::fs::write(&ids, vec![b'x'; 512 * 1024 + 1]).unwrap();
        assert!(Database::load(&ids, &cpu).products.is_empty());
        assert!(cpu_model("model name: AMD One\nmodel name: AMD Two").is_none());
    }
}
