//! Read-only platform capability probe for the CLI composition root.

use std::collections::BTreeSet;

use colosseum_engine::{
    AffinityCapability, AllowedCpuSet, CoreClass, CpuCharacteristics, CpuTopology, NumaNodeId,
    SiblingMapping, affinity_capability, detect_allowed_cpu_set, detect_cpu_characteristics,
    detect_cpu_topology,
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
        schema_version: 1,
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        topology,
        allowed_cpus,
        core_characteristics,
        hard_affinity: affinity_capability(),
    }
}

pub fn print_text(report: &CapabilitiesReport) {
    println!("platform: {} ({})", report.platform, report.architecture);
    match &report.topology.value {
        Some(topology) => {
            println!(
                "topology: available ({} physical cores, {} logical CPUs)",
                topology.physical_core_count, topology.logical_cpu_count
            );
            match &topology.sibling_mapping {
                SiblingMapping::Known { cores } => {
                    println!("SMT sibling map: exact ({} cores)", cores.len());
                }
                SiblingMapping::Unavailable { reason } => {
                    println!("SMT sibling map: unavailable — {reason}");
                }
            }
        }
        None => println!(
            "topology: unavailable — {}",
            report
                .topology
                .reason
                .as_deref()
                .unwrap_or("unknown reason")
        ),
    }
    match &report.allowed_cpus.value {
        Some(AllowedCpuSet::Known { cpus, .. }) => println!(
            "allowed logical CPUs: {} ({})",
            cpus.len(),
            cpus.iter()
                .map(|cpu| format!("{}:{}", cpu.group, cpu.number))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Some(AllowedCpuSet::Unavailable { reason }) => {
            println!("allowed logical CPUs: unavailable — {reason}");
        }
        None => println!(
            "allowed logical CPUs: unavailable — {}",
            report
                .allowed_cpus
                .reason
                .as_deref()
                .unwrap_or("unknown reason")
        ),
    }
    match &report.core_characteristics.value {
        Some(characteristics) => print_characteristics(characteristics),
        None => println!(
            "core class / NUMA: unavailable — {}",
            report
                .core_characteristics
                .reason
                .as_deref()
                .unwrap_or("unknown reason")
        ),
    }
    println!(
        "hard affinity: {}",
        match report.hard_affinity.level {
            colosseum_engine::AffinitySupportLevel::Enforced => "enforced",
            colosseum_engine::AffinitySupportLevel::Unavailable => "unavailable",
        }
    );
    if let Some(mechanism) = &report.hard_affinity.mechanism {
        println!("affinity mechanism: {mechanism}");
    }
    for constraint in &report.hard_affinity.constraints {
        println!("affinity constraint: {constraint}");
    }
    if let Some(reason) = &report.hard_affinity.reason {
        println!("affinity reason: {reason}");
    }
}

fn print_characteristics(characteristics: &CpuCharacteristics) {
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
    let unknown = classes.contains(&CoreClass::Unknown);
    println!(
        "core class / NUMA: available ({} distinct classes{}, {} nodes)",
        classes.len(),
        if unknown { ", including unknown" } else { "" },
        nodes.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_probe_is_serializable_and_names_the_platform() {
        let report = probe();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["platform"], std::env::consts::OS);
        assert!(value["hard_affinity"]["level"].is_string());
    }
}
