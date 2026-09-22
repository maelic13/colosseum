//! Explicitly opt-in platform smoke coverage for hard CPU affinity.
//!
//! Cargo compiles this target only with `platform-smoke`. It spawns real
//! processes, pins them and samples where the operating system schedules
//! them, so it depends on the host: run it on a machine with enforceable
//! affinity (Windows or Linux). Where hard affinity is unavailable it fails
//! rather than passing by skip, because running it is a request for evidence.

use std::collections::BTreeSet;
use std::process::{Child, Command, Stdio};

use colosseum_application::CpuAllocation;
use colosseum_engine::{
    AffinitySupportLevel, AllowedCpuSet, LogicalCpuId, affinity_capability, apply_process_affinity,
    detect_allowed_cpu_set, detect_cpu_topology, process_affinity_groups,
};

#[test]
fn busy_children_reside_only_on_their_enforced_cpu() {
    let capability = affinity_capability();
    assert_eq!(
        capability.level,
        AffinitySupportLevel::Enforced,
        "platform-smoke needs enforceable hard affinity: {}",
        capability
            .reason
            .as_deref()
            .unwrap_or("hard affinity unavailable")
    );
    let topology = detect_cpu_topology().unwrap();
    let allowed_cpus = match detect_allowed_cpu_set(&topology).unwrap() {
        AllowedCpuSet::Known { cpus, .. } => cpus,
        AllowedCpuSet::Unavailable { reason } => {
            panic!("platform-smoke needs the harness's allowed CPU set: {reason}")
        }
    };

    let root = tempfile::tempdir().unwrap();
    let mut children = (0..2)
        .map(|index| {
            let gate = root.path().join(format!("gate-{index}"));
            let child = Command::new(env!("CARGO_BIN_EXE_colosseum-affinity-fixture"))
                .arg(&gate)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let groups = process_affinity_groups(child.id()).unwrap();
            let candidates = allowed_cpus
                .iter()
                .filter(|cpu| groups.contains(&cpu.group))
                .copied()
                .collect::<Vec<_>>();
            assert!(
                !candidates.is_empty(),
                "child groups {groups:?} have no CPU in the harness allowed set"
            );
            let cpu = candidates[index % candidates.len()];
            (child, gate, cpu)
        })
        .collect::<Vec<_>>();

    for (child, gate, cpu) in &children {
        apply_process_affinity(child.id(), &CpuAllocation::Enforced(vec![*cpu])).unwrap();
        std::fs::write(gate, b"sample").unwrap();
    }
    for (child, _, expected) in children.drain(..) {
        assert_child_residency(child, expected);
    }
}

fn assert_child_residency(child: Child, expected: LogicalCpuId) {
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "residency fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (group, number) = line.split_once(':').unwrap();
            LogicalCpuId {
                group: group.parse().unwrap(),
                number: number.parse().unwrap(),
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(observed, BTreeSet::from([expected]));
}
