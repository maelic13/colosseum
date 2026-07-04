// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine runtime model: what kind of binary an engine is, and which
//! translation layer (if any) should launch it.
//!
//! Colosseum runs old Windows-only UCI engines (Rybka, Houdini, …) on every
//! platform by launching them through a translation runtime — Wine on
//! macOS/Linux, the OS-built-in Prism emulation on Windows arm64. This module
//! holds the pure domain types: [`BinaryKind`] (sniffed from the executable's
//! header) and [`EngineRuntime`] (the user's per-engine launcher choice).
//! Resolution to an actual `wine` binary lives in `colosseum-engine`.

use serde::{Deserialize, Serialize};

/// The executable format of an engine binary, sniffed from its file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryKind {
    /// A native executable for the host OS (Mach-O / ELF, or PE on Windows
    /// when the architecture matches the host).
    Native,
    /// A Windows x86-64 PE executable (`PE32+`, machine `AMD64`).
    WindowsX64,
    /// A 32-bit Windows x86 PE executable (`PE32`, machine `i386`).
    WindowsX86,
    /// A Windows arm64 PE executable (machine `ARM64`).
    WindowsArm64,
    /// Format not recognized (scripts, unknown headers, unreadable files).
    Unknown,
}

impl BinaryKind {
    /// Short badge label ("x64", "x86") for non-native Windows binaries;
    /// `None` for native/unknown binaries (no badge shown).
    #[must_use]
    pub fn badge(self) -> Option<&'static str> {
        match self {
            Self::WindowsX64 => Some("x64"),
            Self::WindowsX86 => Some("x86"),
            Self::WindowsArm64 => Some("arm64"),
            Self::Native | Self::Unknown => None,
        }
    }

    /// Whether this is a Windows PE binary of any architecture.
    #[must_use]
    pub fn is_windows(self) -> bool {
        matches!(
            self,
            Self::WindowsX64 | Self::WindowsX86 | Self::WindowsArm64
        )
    }
}

/// Classify an executable from its leading bytes (a few hundred bytes of the
/// file are enough: DOS header + PE headers, or the Mach-O/ELF magic).
///
/// PE binaries always report their PE architecture, even on Windows where they
/// are runnable directly — callers decide what "native for this host" means
/// (see the resolver in `colosseum-engine`).
#[must_use]
pub fn sniff_binary_kind(bytes: &[u8]) -> BinaryKind {
    // ELF (Linux native) and Mach-O (macOS native, thin or fat).
    if bytes.starts_with(&[0x7f, b'E', b'L', b'F']) {
        return BinaryKind::Native;
    }
    if bytes.len() >= 4 {
        let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        // Mach-O thin (feedface/feedfacf ± endianness) and fat (cafebabe/cafebabf).
        if matches!(
            magic,
            0xfeed_face | 0xfeed_facf | 0xcefa_edfe | 0xcffa_edfe | 0xcafe_babe | 0xcafe_babf
        ) {
            return BinaryKind::Native;
        }
    }
    // PE: "MZ" DOS header; e_lfanew at 0x3c points at "PE\0\0" + COFF header,
    // whose first field is the machine type.
    if bytes.len() >= 0x40 && &bytes[0..2] == b"MZ" {
        let e_lfanew =
            u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
        if bytes.len() >= e_lfanew + 6 && &bytes[e_lfanew..e_lfanew + 4] == b"PE\0\0" {
            let machine = u16::from_le_bytes([bytes[e_lfanew + 4], bytes[e_lfanew + 5]]);
            return match machine {
                0x8664 => BinaryKind::WindowsX64,   // IMAGE_FILE_MACHINE_AMD64
                0x014c => BinaryKind::WindowsX86,   // IMAGE_FILE_MACHINE_I386
                0xaa64 => BinaryKind::WindowsArm64, // IMAGE_FILE_MACHINE_ARM64
                _ => BinaryKind::Unknown,
            };
        }
        return BinaryKind::Unknown;
    }
    BinaryKind::Unknown
}

/// The user's per-engine launcher choice, persisted in [`crate::EngineConfig`].
///
/// `Auto` (the default, and the recommended setting) resolves at spawn time:
/// native binaries run directly; Windows binaries on macOS/Linux run through
/// the best available Wine (managed download preferred, then system Wine);
/// Windows binaries on Windows always run directly (Prism handles x64 on
/// arm64 hosts transparently). The explicit variants pin one launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRuntime {
    /// Pick the launcher automatically from the binary kind and what is
    /// installed. The right choice for almost everyone.
    #[default]
    Auto,
    /// Always run the executable directly, even if it looks like a Windows
    /// binary on a non-Windows host.
    Native,
    /// The Wine build downloaded and managed by Colosseum.
    WineManaged,
    /// A Wine installed outside Colosseum (PATH or well-known locations).
    WineSystem,
}

impl EngineRuntime {
    /// Human-readable dropdown label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (recommended)",
            Self::Native => "Native (run directly)",
            Self::WineManaged => "Wine — managed by Colosseum",
            Self::WineSystem => "Wine — system installation",
        }
    }

    /// All selectable variants, in dropdown order.
    pub const ALL: [Self; 4] = [
        Self::Auto,
        Self::Native,
        Self::WineManaged,
        Self::WineSystem,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PE image with the given COFF machine type.
    fn pe_bytes(machine: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x80 + 6];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        b
    }

    #[test]
    fn sniffs_pe_architectures() {
        assert_eq!(sniff_binary_kind(&pe_bytes(0x8664)), BinaryKind::WindowsX64);
        assert_eq!(sniff_binary_kind(&pe_bytes(0x014c)), BinaryKind::WindowsX86);
        assert_eq!(
            sniff_binary_kind(&pe_bytes(0xaa64)),
            BinaryKind::WindowsArm64
        );
        assert_eq!(sniff_binary_kind(&pe_bytes(0x0200)), BinaryKind::Unknown);
    }

    #[test]
    fn sniffs_native_formats() {
        assert_eq!(
            sniff_binary_kind(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]),
            BinaryKind::Native
        );
        // Mach-O 64-bit little-endian magic as stored on disk (cf fa ed fe).
        assert_eq!(
            sniff_binary_kind(&[0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]),
            BinaryKind::Native
        );
        // Fat/universal binary.
        assert_eq!(
            sniff_binary_kind(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2]),
            BinaryKind::Native
        );
    }

    #[test]
    fn mz_without_pe_header_is_unknown() {
        let mut b = vec![0u8; 0x40];
        b[0] = b'M';
        b[1] = b'Z';
        // e_lfanew points beyond the buffer.
        b[0x3c..0x40].copy_from_slice(&0x1000u32.to_le_bytes());
        assert_eq!(sniff_binary_kind(&b), BinaryKind::Unknown);
    }

    #[test]
    fn short_or_garbage_is_unknown() {
        assert_eq!(sniff_binary_kind(&[]), BinaryKind::Unknown);
        assert_eq!(sniff_binary_kind(b"#!/bin/sh"), BinaryKind::Unknown);
        assert_eq!(sniff_binary_kind(b"MZ"), BinaryKind::Unknown);
    }

    #[test]
    fn runtime_default_is_auto() {
        assert_eq!(EngineRuntime::default(), EngineRuntime::Auto);
    }

    #[test]
    fn runtime_serde_round_trip_and_default() {
        let json = serde_json::to_string(&EngineRuntime::WineManaged).unwrap();
        assert_eq!(json, "\"wine_managed\"");
        let back: EngineRuntime = serde_json::from_str(&json).unwrap();
        assert_eq!(back, EngineRuntime::WineManaged);
    }
}
