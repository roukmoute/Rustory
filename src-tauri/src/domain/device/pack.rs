//! Device pack import model — structural validation, no decryption.
//!
//! The import flow ("Copier dans ma bibliothèque") copies the bytes of a
//! `.content/<SHORT_ID>` pack as-is into Rustory's managed storage. This
//! module owns the PURE rules of that acquisition: which entry names are
//! part of the declared supported subset, which bounds apply, and how an
//! enumerated inventory is validated into a deterministic [`PackManifest`].
//! No I/O happens here — the infrastructure reader enumerates the pack
//! into [`PackEntry`] values and copies files; this module only judges.
//!
//! The declared subset is documented in
//! `docs/architecture/device-support-profile.md#Story Import Contract`;
//! a change here without a matching doc update is a bug.

/// Entry names REQUIRED at the pack root. Each must be a non-empty
/// regular file. (Index/list/resource/story-index files of the on-device
/// format — copied verbatim, never parsed.)
pub const REQUIRED_PACK_FILES: [&str; 4] = ["ni", "li", "ri", "si"];

/// Entry names OPTIONAL at the pack root, copied when present.
pub const OPTIONAL_PACK_FILES: [&str; 2] = ["nm", "bt"];

/// Asset tree directories allowed at the pack root.
pub const PACK_ASSET_DIRS: [&str; 2] = ["rf", "sf"];

/// OS cruft skipped silently (never copied, never a refusal cause).
/// Compared case-insensitively: the source volume is FAT (case-insensitive),
/// so `THUMBS.DB` and `Thumbs.db` are the same on-device file.
pub const OS_CRUFT_NAMES: [&str; 2] = ["thumbs.db", ".ds_store"];

/// AppleDouble resource-fork prefix (`._*`), skipped like the names above.
pub const OS_CRUFT_PREFIX: &str = "._";

/// Hard cap on the total byte size of an importable pack.
pub const MAX_IMPORT_PACK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Hard cap on the number of files an importable pack may contain.
pub const MAX_IMPORT_PACK_FILES: usize = 4096;

/// Maximum depth of a file BELOW an asset tree root (`rf/000/x` = 2).
pub const MAX_PACK_ASSET_DEPTH: usize = 2;

/// Kind of an enumerated pack entry, as observed via `symlink_metadata`
/// (the infrastructure layer must NOT follow symlinks while classifying).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackEntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// One enumerated entry of a pack folder. `rel_path` is the
/// forward-slash-separated path relative to the pack root (the
/// `.content/<SHORT_ID>` directory), never absolute, never containing
/// `.` / `..` components — the enumerator builds it from component names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    pub rel_path: String,
    pub kind: PackEntryKind,
    pub size: u64,
}

/// One file retained by validation, in its staging-relative location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    pub rel_path: String,
    pub size: u64,
}

/// Deterministic description of a validated pack: the retained files
/// sorted by `rel_path` (lexicographic, byte order) plus the total byte
/// size. The sorted order is the base of the aggregate checksum computed
/// by the infrastructure layer — two enumerations of the same pack MUST
/// produce the same manifest regardless of directory iteration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackManifest {
    pub files: Vec<PackFile>,
    pub total_bytes: u64,
}

/// Exhaustive refusal causes for a pack inventory. Each maps to a
/// `details.source` of the wire `IMPORT_FAILED` taxonomy: every variant
/// is `pack_invalid` except the size/count bounds which are
/// `pack_oversize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackValidationIssue {
    /// A required root file is absent.
    MissingRequired { name: &'static str },
    /// A required root file exists but is empty.
    EmptyRequired { name: &'static str },
    /// An entry outside the declared supported subset.
    UnknownEntry { rel_path: String },
    /// A symlink or special file anywhere in the pack.
    NotARegularFile { rel_path: String },
    /// A file nested deeper than [`MAX_PACK_ASSET_DEPTH`] below its tree.
    TooDeep { rel_path: String },
    /// More files than [`MAX_IMPORT_PACK_FILES`].
    TooManyFiles { count: usize },
    /// Total bytes beyond [`MAX_IMPORT_PACK_BYTES`].
    TooLarge { total_bytes: u64 },
}

impl PackValidationIssue {
    /// Stable `details.source` token of the wire taxonomy.
    pub const fn source_tag(&self) -> &'static str {
        match self {
            Self::TooManyFiles { .. } | Self::TooLarge { .. } => "pack_oversize",
            _ => "pack_invalid",
        }
    }
}

/// Is this basename OS cruft to skip silently? Case-insensitive on the
/// names (FAT source volume) plus the AppleDouble `._` prefix.
pub fn is_os_cruft(basename: &str) -> bool {
    if basename.starts_with(OS_CRUFT_PREFIX) {
        return true;
    }
    let lower = basename.to_ascii_lowercase();
    OS_CRUFT_NAMES.contains(&lower.as_str())
}

/// Validate an enumerated pack inventory against the declared supported
/// subset. Pure: consumes the entry list the infrastructure enumerated,
/// returns either the deterministic manifest of files to copy or the
/// FIRST refusal encountered (all-or-nothing — a single violation refuses
/// the whole pack, no blind partial copy).
pub fn validate_pack_inventory(entries: &[PackEntry]) -> Result<PackManifest, PackValidationIssue> {
    let mut files: Vec<PackFile> = Vec::new();
    let mut total_bytes: u64 = 0;

    for entry in entries {
        let components: Vec<&str> = entry.rel_path.split('/').collect();
        let basename = components.last().copied().unwrap_or("");

        // OS cruft is skipped at ANY depth — never copied, never fatal.
        if is_os_cruft(basename) {
            continue;
        }

        match entry.kind {
            PackEntryKind::Symlink | PackEntryKind::Other => {
                return Err(PackValidationIssue::NotARegularFile {
                    rel_path: entry.rel_path.clone(),
                });
            }
            PackEntryKind::Dir => {
                // Directories carry no bytes; their placement is judged. A
                // root dir must be a declared asset tree; a nested dir must
                // live under one. A dir deeper than MAX_PACK_ASSET_DEPTH is
                // refused OUTRIGHT: any file inside it would be too deep
                // anyway, and refusing the dir lets the enumerator stop
                // recursing at a bounded depth without ever silently
                // skipping content (AC "jamais silencieusement incomplet").
                if !PACK_ASSET_DIRS.contains(&components[0]) {
                    return Err(PackValidationIssue::UnknownEntry {
                        rel_path: entry.rel_path.clone(),
                    });
                }
                if components.len() > MAX_PACK_ASSET_DEPTH {
                    return Err(PackValidationIssue::TooDeep {
                        rel_path: entry.rel_path.clone(),
                    });
                }
            }
            PackEntryKind::File => {
                if components.len() == 1 {
                    let name = components[0];
                    let known =
                        REQUIRED_PACK_FILES.contains(&name) || OPTIONAL_PACK_FILES.contains(&name);
                    if !known {
                        return Err(PackValidationIssue::UnknownEntry {
                            rel_path: entry.rel_path.clone(),
                        });
                    }
                } else {
                    if !PACK_ASSET_DIRS.contains(&components[0]) {
                        return Err(PackValidationIssue::UnknownEntry {
                            rel_path: entry.rel_path.clone(),
                        });
                    }
                    // Depth below the asset tree root: `rf/x` = 1,
                    // `rf/000/x` = 2, `rf/a/b/x` = 3 (refused).
                    let depth_below_tree = components.len() - 1;
                    if depth_below_tree > MAX_PACK_ASSET_DEPTH {
                        return Err(PackValidationIssue::TooDeep {
                            rel_path: entry.rel_path.clone(),
                        });
                    }
                }
                files.push(PackFile {
                    rel_path: entry.rel_path.clone(),
                    size: entry.size,
                });
                total_bytes = total_bytes.saturating_add(entry.size);
            }
        }
    }

    for name in REQUIRED_PACK_FILES {
        let found = files
            .iter()
            .find(|f| f.rel_path == name)
            .ok_or(PackValidationIssue::MissingRequired { name })?;
        if found.size == 0 {
            return Err(PackValidationIssue::EmptyRequired { name });
        }
    }

    if files.len() > MAX_IMPORT_PACK_FILES {
        return Err(PackValidationIssue::TooManyFiles { count: files.len() });
    }
    if total_bytes > MAX_IMPORT_PACK_BYTES {
        return Err(PackValidationIssue::TooLarge { total_bytes });
    }

    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    Ok(PackManifest { files, total_bytes })
}

/// Default title of the local draft created by a device copy. The opaque
/// short identifier carries the provenance ("Histoire de ma Lunii
/// (FAC5562D)"); the user can rename immediately in the editor. MUST pass
/// `validate_title` for every possible 8-hex shortId — asserted by test.
pub fn imported_story_title(short_id: &str) -> String {
    format!("Histoire de ma Lunii ({short_id})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::{normalize_title, validate_title};

    fn file(rel_path: &str, size: u64) -> PackEntry {
        PackEntry {
            rel_path: rel_path.into(),
            kind: PackEntryKind::File,
            size,
        }
    }

    fn dir(rel_path: &str) -> PackEntry {
        PackEntry {
            rel_path: rel_path.into(),
            kind: PackEntryKind::Dir,
            size: 0,
        }
    }

    fn plausible_pack() -> Vec<PackEntry> {
        vec![
            file("ni", 512),
            file("li", 256),
            file("ri", 128),
            file("si", 128),
            file("nm", 32),
            file("bt", 64),
            dir("rf"),
            dir("rf/000"),
            file("rf/000/AAAAAAAA", 2048),
            dir("sf"),
            dir("sf/000"),
            file("sf/000/BBBBBBBB", 4096),
        ]
    }

    #[test]
    fn validates_a_plausible_full_pack() {
        let manifest = validate_pack_inventory(&plausible_pack()).expect("valid");
        assert_eq!(manifest.files.len(), 8);
        assert_eq!(
            manifest.total_bytes,
            512 + 256 + 128 + 128 + 32 + 64 + 2048 + 4096
        );
    }

    #[test]
    fn validates_a_minimal_pack_with_only_required_files() {
        let entries = vec![file("ni", 1), file("li", 1), file("ri", 1), file("si", 1)];
        let manifest = validate_pack_inventory(&entries).expect("valid");
        assert_eq!(manifest.files.len(), 4);
        assert_eq!(manifest.total_bytes, 4);
    }

    #[test]
    fn manifest_is_sorted_lexicographically_regardless_of_input_order() {
        let mut entries = plausible_pack();
        entries.reverse();
        let manifest = validate_pack_inventory(&entries).expect("valid");
        let paths: Vec<&str> = manifest.files.iter().map(|f| f.rel_path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "manifest order must be deterministic");
    }

    #[test]
    fn rejects_missing_required_file() {
        let entries = vec![file("ni", 1), file("li", 1), file("ri", 1)];
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(issue, PackValidationIssue::MissingRequired { name: "si" });
        assert_eq!(issue.source_tag(), "pack_invalid");
    }

    #[test]
    fn rejects_empty_required_file() {
        let entries = vec![file("ni", 0), file("li", 1), file("ri", 1), file("si", 1)];
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(issue, PackValidationIssue::EmptyRequired { name: "ni" });
    }

    #[test]
    fn rejects_unknown_root_file() {
        let mut entries = plausible_pack();
        entries.push(file("evil.bin", 10));
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::UnknownEntry {
                rel_path: "evil.bin".into()
            }
        );
    }

    #[test]
    fn rejects_unknown_root_directory() {
        let mut entries = plausible_pack();
        entries.push(dir("etc"));
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::UnknownEntry {
                rel_path: "etc".into()
            }
        );
    }

    #[test]
    fn rejects_file_nested_under_unknown_directory() {
        let entries = vec![
            file("ni", 1),
            file("li", 1),
            file("ri", 1),
            file("si", 1),
            file("data/evil", 10),
        ];
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::UnknownEntry {
                rel_path: "data/evil".into()
            }
        );
    }

    #[test]
    fn skips_os_cruft_at_any_depth_without_copying_it() {
        let mut entries = plausible_pack();
        entries.push(file("Thumbs.db", 10));
        entries.push(file("THUMBS.DB", 10));
        entries.push(file(".DS_Store", 10));
        entries.push(file("rf/000/._resource", 10));
        let manifest = validate_pack_inventory(&entries).expect("cruft must not refuse");
        assert!(
            manifest.files.iter().all(|f| !f.rel_path.contains("Thumbs")
                && !f.rel_path.contains("DS_Store")
                && !f.rel_path.contains("._")),
            "cruft must never land in the manifest: {:?}",
            manifest.files
        );
        assert_eq!(manifest.files.len(), 8, "only the declared files remain");
    }

    #[test]
    fn rejects_symlink_anywhere() {
        let mut entries = plausible_pack();
        entries.push(PackEntry {
            rel_path: "rf/000/link".into(),
            kind: PackEntryKind::Symlink,
            size: 0,
        });
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::NotARegularFile {
                rel_path: "rf/000/link".into()
            }
        );
    }

    #[test]
    fn rejects_special_file_anywhere() {
        let mut entries = plausible_pack();
        entries.push(PackEntry {
            rel_path: "rf/fifo".into(),
            kind: PackEntryKind::Other,
            size: 0,
        });
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert!(matches!(issue, PackValidationIssue::NotARegularFile { .. }));
    }

    #[test]
    fn accepts_asset_file_at_depth_one_and_two() {
        let mut entries = vec![file("ni", 1), file("li", 1), file("ri", 1), file("si", 1)];
        entries.push(file("rf/direct", 1)); // depth 1
        entries.push(file("sf/000/nested", 1)); // depth 2
        let manifest = validate_pack_inventory(&entries).expect("depth ≤ 2 is valid");
        assert_eq!(manifest.files.len(), 6);
    }

    #[test]
    fn rejects_asset_file_deeper_than_two_levels() {
        let mut entries = plausible_pack();
        entries.push(file("rf/a/b/too-deep", 1)); // depth 3
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::TooDeep {
                rel_path: "rf/a/b/too-deep".into()
            }
        );
    }

    #[test]
    fn rejects_directory_deeper_than_asset_depth_even_when_empty() {
        // A dir at `rf/a/b` could only host too-deep files; refusing the
        // dir itself lets the enumerator stop recursing at a bounded depth
        // without ever silently skipping nested content.
        let mut entries = plausible_pack();
        entries.push(dir("rf/a"));
        entries.push(dir("rf/a/b"));
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert_eq!(
            issue,
            PackValidationIssue::TooDeep {
                rel_path: "rf/a/b".into()
            }
        );
    }

    #[test]
    fn rejects_pack_with_too_many_files() {
        let mut entries = vec![file("ni", 1), file("li", 1), file("ri", 1), file("si", 1)];
        for i in 0..(MAX_IMPORT_PACK_FILES - 3) {
            entries.push(file(&format!("rf/000/{i:08X}"), 1));
        }
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        match issue {
            PackValidationIssue::TooManyFiles { count } => {
                assert!(count > MAX_IMPORT_PACK_FILES);
            }
            other => panic!("expected TooManyFiles, got {other:?}"),
        }
        assert_eq!(issue.source_tag(), "pack_oversize");
    }

    #[test]
    fn rejects_pack_larger_than_byte_bound() {
        let entries = vec![
            file("ni", 1),
            file("li", 1),
            file("ri", 1),
            file("si", 1),
            file("rf/000/huge", MAX_IMPORT_PACK_BYTES),
        ];
        let issue = validate_pack_inventory(&entries).expect_err("must refuse");
        assert!(matches!(issue, PackValidationIssue::TooLarge { .. }));
        assert_eq!(issue.source_tag(), "pack_oversize");
    }

    #[test]
    fn total_bytes_saturates_instead_of_overflowing() {
        let entries = vec![
            file("ni", u64::MAX),
            file("li", u64::MAX),
            file("ri", 1),
            file("si", 1),
        ];
        let issue = validate_pack_inventory(&entries).expect_err("must refuse oversize");
        assert!(matches!(issue, PackValidationIssue::TooLarge { .. }));
    }

    #[test]
    fn bounds_match_published_contract() {
        assert_eq!(MAX_IMPORT_PACK_BYTES, 2 * 1024 * 1024 * 1024);
        assert_eq!(MAX_IMPORT_PACK_FILES, 4096);
        assert_eq!(MAX_PACK_ASSET_DEPTH, 2);
    }

    #[test]
    fn is_os_cruft_matches_documented_set() {
        assert!(is_os_cruft("Thumbs.db"));
        assert!(is_os_cruft("THUMBS.DB"));
        assert!(is_os_cruft(".DS_Store"));
        assert!(is_os_cruft("._AppleDouble"));
        assert!(!is_os_cruft("ni"));
        assert!(!is_os_cruft("rf"));
        assert!(!is_os_cruft("nm"));
    }

    #[test]
    fn imported_story_title_passes_title_validation_for_any_8_hex_short_id() {
        for short_id in ["FAC5562D", "00000000", "FFFFFFFF", "0A1B2C3D"] {
            let title = imported_story_title(short_id);
            assert_eq!(title, format!("Histoire de ma Lunii ({short_id})"));
            let normalized = normalize_title(&title);
            assert_eq!(
                normalized, title,
                "the default title must already be normalized (NFC, no surrounding spaces)"
            );
            validate_title(&normalized).unwrap_or_else(|err| {
                panic!("default title for {short_id} must pass validate_title: {err:?}")
            });
        }
    }
}
