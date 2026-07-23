/// Per-profile authorization map. Each field expresses whether THIS
/// profile is allowed to perform the operation, regardless of whether
/// the implementation exists yet. The Rust capability gate must consult
/// this map BEFORE any mutation, never the inverse (NFR17 + NFR18).
///
/// All defaults are `false` (fail-closed). A new operation ADDS a field;
/// a new profile ADDS a registry entry. No accidental write authorization
/// can be introduced by typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedOperations {
    /// Read the device-side library.
    pub read_library: bool,
    /// Inspect a single device-side story before import.
    pub inspect_story: bool,
    /// Import a device-side story into the local library.
    pub import_story: bool,
    /// Write any byte to the device. MUST stay `false` until the
    /// transfer pipeline wires the real gate — hard-coded `false`
    /// here is the fail-closed default.
    pub write_story: bool,
    /// Delete a story already on the device (delist its `.pi` entry and
    /// remove its content folder). A DEVICE MUTATION, gated separately
    /// from `write_story`: deletion removes opaque bytes and needs no
    /// pack-format ciphering, so a cohort can be allowed to delete before
    /// it can be written to (e.g. Lunii V3).
    pub delete_story: bool,
    /// Send a STUdio-format pack archive (`.zip`) to the device: transcode
    /// its graph to the on-device index files, cipher them with the target
    /// device's own content key and write the pack. A DEVICE MUTATION,
    /// gated separately from `write_story` (the round-trip of an imported
    /// pack): the archive-send owns its whole pipeline, so a cohort can be
    /// allowed to receive archives before the round-trip is (Lunii V3).
    pub send_archive: bool,
}

impl SupportedOperations {
    /// All-false constant, used as the unambiguous fail-closed baseline.
    /// Tests assert that synthesizing a profile through this constant
    /// blocks every operation.
    pub const ALL_FALSE: Self = Self {
        read_library: false,
        inspect_story: false,
        import_story: false,
        write_story: false,
        delete_story: false,
        send_archive: false,
    };

    /// Lookup helper used by the capability gate to ask in a typed way:
    /// "is operation X allowed on this profile?". Avoids stringly-typed
    /// names (`&str` would let a typo silently authorize an operation).
    pub fn allows(&self, op: SupportedOperation) -> bool {
        match op {
            SupportedOperation::ReadLibrary => self.read_library,
            SupportedOperation::InspectStory => self.inspect_story,
            SupportedOperation::ImportStory => self.import_story,
            SupportedOperation::WriteStory => self.write_story,
            SupportedOperation::DeleteStory => self.delete_story,
            SupportedOperation::SendArchive => self.send_archive,
        }
    }
}

/// Typed mirror of the `SupportedOperations` fields. The capability gate
/// (`application::device::check_operation_allowed`) accepts this enum so
/// callers cannot pass an arbitrary string and bypass the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedOperation {
    ReadLibrary,
    InspectStory,
    ImportStory,
    WriteStory,
    DeleteStory,
    SendArchive,
}

impl SupportedOperation {
    /// Stable diagnostic tag for logs / error details. Closed set.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::ReadLibrary => "read_library",
            Self::InspectStory => "inspect_story",
            Self::ImportStory => "import_story",
            Self::WriteStory => "write_story",
            Self::DeleteStory => "delete_story",
            Self::SendArchive => "send_archive",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_false_blocks_every_operation() {
        let ops = SupportedOperations::ALL_FALSE;
        assert!(!ops.allows(SupportedOperation::ReadLibrary));
        assert!(!ops.allows(SupportedOperation::InspectStory));
        assert!(!ops.allows(SupportedOperation::ImportStory));
        assert!(!ops.allows(SupportedOperation::WriteStory));
        assert!(!ops.allows(SupportedOperation::DeleteStory));
        assert!(!ops.allows(SupportedOperation::SendArchive));
    }

    #[test]
    fn allows_returns_true_only_for_authorized_operation() {
        let ops = SupportedOperations {
            read_library: true,
            inspect_story: false,
            import_story: false,
            write_story: false,
            delete_story: false,
            send_archive: false,
        };
        assert!(ops.allows(SupportedOperation::ReadLibrary));
        assert!(!ops.allows(SupportedOperation::InspectStory));
        assert!(!ops.allows(SupportedOperation::ImportStory));
        assert!(!ops.allows(SupportedOperation::WriteStory));
        assert!(!ops.allows(SupportedOperation::DeleteStory));
        assert!(!ops.allows(SupportedOperation::SendArchive));
    }

    #[test]
    fn send_archive_authorizes_only_the_archive_send() {
        let ops = SupportedOperations {
            send_archive: true,
            ..SupportedOperations::ALL_FALSE
        };
        assert!(ops.allows(SupportedOperation::SendArchive));
        assert!(!ops.allows(SupportedOperation::WriteStory));
        assert!(!ops.allows(SupportedOperation::DeleteStory));
    }

    #[test]
    fn diagnostic_tags_are_stable() {
        assert_eq!(
            SupportedOperation::ReadLibrary.diagnostic_tag(),
            "read_library"
        );
        assert_eq!(
            SupportedOperation::InspectStory.diagnostic_tag(),
            "inspect_story"
        );
        assert_eq!(
            SupportedOperation::ImportStory.diagnostic_tag(),
            "import_story"
        );
        assert_eq!(
            SupportedOperation::WriteStory.diagnostic_tag(),
            "write_story"
        );
        assert_eq!(
            SupportedOperation::DeleteStory.diagnostic_tag(),
            "delete_story"
        );
        assert_eq!(
            SupportedOperation::SendArchive.diagnostic_tag(),
            "send_archive"
        );
    }
}
