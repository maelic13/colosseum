//! Read-only platform capability probe for the CLI composition root.

use std::collections::BTreeSet;

use colosseum_engine::{
    AffinityCapability, AllowedCpuSet, CacheDomainId, CoreClass, CpuCharacteristics, CpuTopology,
    NumaNodeId, SiblingMapping, affinity_capability, detect_allowed_cpu_set,
    detect_cpu_characteristics, detect_cpu_topology,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct Probe<T> {
    pub status: ProbeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl<T> Probe<T> {
    fn available(value: T) -> Self {
        Self {
            status: ProbeStatus::Available,
            value: Some(value),
            reason: None,
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: ProbeStatus::Unavailable,
            value: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CapabilitiesReport {
    pub schema_version: u32,
    pub platform: &'static str,
    pub architecture: &'static str,
    pub topology: Probe<CpuTopology>,
    pub allowed_cpus: Probe<AllowedCpuSet>,
    pub core_characteristics: Probe<CpuCharacteristics>,
    pub hard_affinity: AffinityCapability,
}

#[must_use]
pub fn probe() -> CapabilitiesReport {
    let topology = match detect_cpu_topology() {
        Ok(topology) => Probe::available(topology),
        Err(error) => Probe::unavailable(error.to_string()),
    };
    let (allowed_cpus, core_characteristics) = if let Some(topology) = &topology.value {
        let allowed = match detect_allowed_cpu_set(topology) {
            Ok(AllowedCpuSet::Known { source, cpus }) => {
                Probe::available(AllowedCpuSet::Known { source, cpus })
            }
            Ok(AllowedCpuSet::Unavailable { reason }) => Probe::unavailable(reason),
            Err(error) => Probe::unavailable(error.to_string()),
        };
        let characteristics = match detect_cpu_characteristics(topology) {
            Ok(characteristics) => Probe::available(characteristics),
            Err(error) => Probe::unavailable(error.to_string()),
        };
        (allowed, characteristics)
    } else {
        let reason = "topology detection is unavailable";
        (Probe::unavailable(reason), Probe::unavailable(reason))
    };

    CapabilitiesReport {
        schema_version: 2,
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        topology,
        allowed_cpus,
        core_characteristics,
        hard_affinity: affinity_capability(),
    }
}

pub fn print_text(report: &CapabilitiesReport) {
    print!("{}", render_text(report));
}

/// Render the human-readable probe.
///
/// This is a pure function so every branch can be asserted on every platform.
/// The headings are a contract: a host that supplies none of a fact still
/// prints its line with the reason, because a reader scanning for topology,
/// core class or cache domains must find an answer rather than nothing.
#[must_use]
pub fn render_text(report: &CapabilitiesReport) -> String {
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(format!(
        "platform: {} ({})",
        report.platform, report.architecture
    ));
    match &report.topology.value {
        Some(topology) => {
            line(format!(
                "topology: available ({} physical cores, {} logical CPUs)",
                topology.physical_core_count, topology.logical_cpu_count
            ));
            match &topology.sibling_mapping {
                SiblingMapping::Known { cores } => {
                    line(format!("SMT sibling map: exact ({} cores)", cores.len()));
                }
                SiblingMapping::Unavailable { reason } => {
                    line(format!("SMT sibling map: unavailable — {reason}"));
                }
            }
        }
        None => line(format!(
            "topology: unavailable — {}",
            reason_of(&report.topology.reason)
        )),
    }
    match &report.allowed_cpus.value {
        Some(AllowedCpuSet::Known { cpus, .. }) => line(format!(
            "allowed logical CPUs: {} ({})",
            cpus.len(),
            cpus.iter()
                .map(|cpu| format!("{}:{}", cpu.group, cpu.number))
                .collect::<Vec<_>>()
                .join(",")
        )),
        Some(AllowedCpuSet::Unavailable { reason }) => {
            line(format!("allowed logical CPUs: unavailable — {reason}"));
        }
        None => line(format!(
            "allowed logical CPUs: unavailable — {}",
            reason_of(&report.allowed_cpus.reason)
        )),
    }
    match &report.core_characteristics.value {
        Some(characteristics) => describe_characteristics(characteristics, &mut line),
        None => {
            let reason = reason_of(&report.core_characteristics.reason).to_owned();
            line(format!("core class / NUMA: unavailable — {reason}"));
            line(format!("last-level cache domains: unavailable — {reason}"));
        }
    }
    line(format!(
        "hard affinity: {}",
        match report.hard_affinity.level {
            colosseum_engine::AffinitySupportLevel::Enforced => "enforced",
            colosseum_engine::AffinitySupportLevel::Unavailable => "unavailable",
        }
    ));
    if let Some(mechanism) = &report.hard_affinity.mechanism {
        line(format!("affinity mechanism: {mechanism}"));
    }
    for constraint in &report.hard_affinity.constraints {
        line(format!("affinity constraint: {constraint}"));
    }
    if let Some(reason) = &report.hard_affinity.reason {
        line(format!("affinity reason: {reason}"));
    }
    out
}

fn reason_of(reason: &Option<String>) -> &str {
    reason.as_deref().unwrap_or("unknown reason")
}

fn describe_characteristics(characteristics: &CpuCharacteristics, line: &mut impl FnMut(String)) {
    let classes = characteristics
        .cores
        .iter()
        .map(|core| core.core_class)
        .collect::<BTreeSet<CoreClass>>();
    let nodes = characteristics
        .cores
        .iter()
        .filter_map(|core| core.numa_node)
        .collect::<BTreeSet<NumaNodeId>>();
    let domains = characteristics
        .cores
        .iter()
        .filter_map(|core| core.last_level_cache)
        .collect::<BTreeSet<CacheDomainId>>();
    let unreported = characteristics
        .cores
        .iter()
        .filter(|core| core.last_level_cache.is_none())
        .count();
    let unknown = classes.contains(&CoreClass::Unknown);
    line(format!(
        "core class / NUMA: available ({} distinct classes{}, {} nodes)",
        classes.len(),
        if unknown { ", including unknown" } else { "" },
        nodes.len()
    ));
    line(format!(
        "last-level cache domains: {}",
        if domains.is_empty() {
            "none reported".to_string()
        } else {
            format!(
                "{} ({})",
                domains.len(),
                domains
                    .iter()
                    .map(|domain| format!("L{} #{}", domain.level, domain.index))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ));
    if unreported > 0 {
        line(format!(
            "cores without a reported cache domain: {unreported}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every heading a reader scans for, on every host.
    const HEADINGS: [&str; 6] = [
        "platform:",
        "topology:",
        "allowed logical CPUs:",
        "core class / NUMA:",
        "last-level cache domains:",
        "hard affinity:",
    ];

    fn unavailable_report() -> CapabilitiesReport {
        // A host like macOS: counts only, so no logical identity and therefore
        // no class, node or cache evidence at all.
        CapabilitiesReport {
            schema_version: 2,
            platform: "macos",
            architecture: "aarch64",
            topology: Probe::available(CpuTopology {
                source: colosseum_engine::TopologySource::MacOsSysctlCounts,
                physical_core_count: 3,
                logical_cpu_count: 3,
                sibling_mapping: SiblingMapping::Unavailable {
                    reason: "counts only".into(),
                },
            }),
            allowed_cpus: Probe::unavailable("no logical CPU identities"),
            core_characteristics: Probe::unavailable("logical CPU identities are unavailable"),
            hard_affinity: colosseum_engine::affinity_capability(),
        }
    }

    /// The regression this exists for: a host that reports no core
    /// characteristics used to print no cache-domain line at all, so the fact
    /// was missing rather than reported unavailable.
    #[test]
    fn a_host_without_any_placement_evidence_still_reports_every_heading() {
        let text = render_text(&unavailable_report());
        for heading in HEADINGS {
            assert!(text.contains(heading), "missing {heading} in\n{text}");
        }
        assert!(
            text.contains("last-level cache domains: unavailable — logical CPU identities"),
            "{text}"
        );
    }

    #[test]
    fn this_host_reports_every_heading_whatever_it_supports() {
        let text = render_text(&probe());
        for heading in HEADINGS {
            assert!(text.contains(heading), "missing {heading} in\n{text}");
        }
    }

    #[test]
    fn host_probe_is_serializable_and_names_the_platform() {
        let report = probe();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["platform"], std::env::consts::OS);
        assert!(value["hard_affinity"]["level"].is_string());
    }

    /// Class, NUMA node and last-level cache domain are all reportable fields;
    /// a core the operating system said nothing about stays explicitly null.
    #[test]
    fn host_probe_reports_class_numa_and_cache_domain_per_core() {
        let report = probe();
        let value = serde_json::to_value(&report).unwrap();
        let Some(cores) = value["core_characteristics"]["value"]["cores"].as_array() else {
            assert_eq!(value["core_characteristics"]["status"], "unavailable");
            return;
        };
        for core in cores {
            assert!(core["core_class"].is_object());
            assert!(core["numa_node"].is_object() || core["numa_node"].is_null());
            assert!(
                core["last_level_cache"].is_object() || core["last_level_cache"].is_null(),
                "{core}"
            );
        }
    }
}
