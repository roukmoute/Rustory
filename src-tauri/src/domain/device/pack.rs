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
    /// An OPAQUE pack (FLAM) holding zero files — nothing to acquire.
    EmptyPack,
    /// A directory inside an OPAQUE pack with no file anywhere below it.
    /// The manifest/checksum represent FILES only, so an empty directory
    /// cannot round-trip — the honest all-or-nothing contract refuses it
    /// rather than silently importing an altered tree.
    EmptyDirectory { rel_path: String },
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

/// Validate an enumerated OPAQUE pack inventory (FLAM — the internal
/// format is publicly unknown, so any entry-name whitelist would be an
/// invention). STRUCTURAL rules only, all-or-nothing, born stricter than
/// the historical Lunii walker (see
/// `device-support-profile.md#FLAM library inventory & story import`):
///
/// - every entry must be a regular file or a real directory — a symlink
///   or special file refuses the whole pack;
/// - NO name whitelist, NO OS-cruft skip: every regular file is retained
///   verbatim (the pack is opaque);
/// - the Lunii bounds are REUSED: [`MAX_IMPORT_PACK_FILES`],
///   [`MAX_IMPORT_PACK_BYTES`], and the same numeric depth rule
///   ([`MAX_PACK_ASSET_DEPTH`] — a directory beyond it, or a file nested
///   deeper than a 2-level tree, refuses);
/// - a pack holding ZERO files refuses ([`PackValidationIssue::EmptyPack`]).
pub fn validate_opaque_pack_inventory(
    entries: &[PackEntry],
) -> Result<PackManifest, PackValidationIssue> {
    let mut files: Vec<PackFile> = Vec::new();
    let mut total_bytes: u64 = 0;

    for entry in entries {
        let component_count = entry.rel_path.split('/').count();
        match entry.kind {
            PackEntryKind::Symlink | PackEntryKind::Other => {
                return Err(PackValidationIssue::NotARegularFile {
                    rel_path: entry.rel_path.clone(),
                });
            }
            PackEntryKind::Dir => {
                // Same numeric rule as the Lunii asset trees: a directory
                // deeper than MAX_PACK_ASSET_DEPTH is refused OUTRIGHT so
                // the enumerator can stop recursing at a bounded depth
                // without ever silently skipping content.
                if component_count > MAX_PACK_ASSET_DEPTH {
                    return Err(PackValidationIssue::TooDeep {
                        rel_path: entry.rel_path.clone(),
                    });
                }
            }
            PackEntryKind::File => {
                // A root file has 1 component; a file inside a 2-level
                // tree has 3. Deeper is refused — the same ceiling the
                // Lunii `rf/000/x` shape sits exactly under.
                if component_count > MAX_PACK_ASSET_DEPTH + 1 {
                    return Err(PackValidationIssue::TooDeep {
                        rel_path: entry.rel_path.clone(),
                    });
                }
                files.push(PackFile {
                    rel_path: entry.rel_path.clone(),
                    size: entry.size,
                });
                total_bytes = total_bytes.saturating_add(entry.size);
            }
        }
    }

    if files.is_empty() {
        return Err(PackValidationIssue::EmptyPack);
    }
    // A directory with NO file anywhere below it (a leaf-empty dir, or a
    // dir holding only empty dirs) cannot round-trip: the manifest and
    // the staging represent files only. Refuse it — the imported tree
    // must be the source tree or nothing (all-or-nothing).
    for entry in entries {
        if entry.kind != PackEntryKind::Dir {
            continue;
        }
        let prefix = format!("{}/", entry.rel_path);
        let holds_a_file = files.iter().any(|f| f.rel_path.starts_with(&prefix));
        if !holds_a_file {
            return Err(PackValidationIssue::EmptyDirectory {
                rel_path: entry.rel_path.clone(),
            });
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
/// short identifier carries the provenance and the wording is
/// FAMILY-CORRECT ("Histoire de ma Lunii (FAC5562D)" verbatim for a
/// Lunii, "Histoire de mon FLAM (FAC5562D)" for a FLAM — Change Control,
/// product-language.md); the user can rename immediately in the editor.
/// MUST pass `validate_title` for every possible 8-hex shortId and both
/// families — asserted by test.
pub fn imported_story_title(family: super::DeviceFamily, short_id: &str) -> String {
    match family {
        super::DeviceFamily::Lunii => format!("Histoire de ma Lunii ({short_id})"),
        super::DeviceFamily::Flam => format!("Histoire de mon FLAM ({short_id})"),
    }
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
        use crate::domain::device::DeviceFamily;
        for short_id in ["FAC5562D", "00000000", "FFFFFFFF", "0A1B2C3D"] {
            for (family, expected) in [
                (
                    DeviceFamily::Lunii,
                    format!("Histoire de ma Lunii ({short_id})"),
                ),
                (
                    DeviceFamily::Flam,
                    format!("Histoire de mon FLAM ({short_id})"),
                ),
            ] {
                let title = imported_story_title(family, short_id);
                assert_eq!(title, expected);
                let normalized = normalize_title(&title);
                assert_eq!(
                    normalized, title,
                    "the default title must already be normalized (NFC, no surrounding spaces)"
                );
                validate_title(&normalized).unwrap_or_else(|err| {
                    panic!(
                        "default title for {family:?}/{short_id} must pass validate_title: {err:?}"
                    )
                });
            }
        }
    }

    #[test]
    fn imported_story_title_lunii_literal_stays_verbatim() {
        // AC2 family isolation: the Lunii default title does not change
        // by a byte with the FLAM extension.
        use crate::domain::device::DeviceFamily;
        assert_eq!(
            imported_story_title(DeviceFamily::Lunii, "FAC5562D"),
            "Histoire de ma Lunii (FAC5562D)"
        );
    }

    // ---- validate_opaque_pack_inventory (FLAM — structural only) ----

    #[test]
    fn opaque_validation_retains_every_regular_file_without_whitelist() {
        // Arbitrary names (unknown format): everything regular is
        // retained verbatim — even names the Lunii subset would refuse.
        let entries = vec![
            file("whatever.bin", 128),
            file("Thumbs.db", 16), // no cruft skip: the pack is opaque
            dir("data"),
            file("data/chunk", 64),
        ];
        let manifest = validate_opaque_pack_inventory(&entries).expect("valid");
        assert_eq!(manifest.files.len(), 3);
        assert_eq!(manifest.total_bytes, 208);
        // Deterministic order: sorted by rel_path.
        assert_eq!(manifest.files[0].rel_path, "Thumbs.db");
        assert_eq!(manifest.files[1].rel_path, "data/chunk");
        assert_eq!(manifest.files[2].rel_path, "whatever.bin");
    }

    #[test]
    fn opaque_validation_refuses_a_symlink_anywhere() {
        let entries = vec![
            file("a", 1),
            PackEntry {
                rel_path: "link".into(),
                kind: PackEntryKind::Symlink,
                size: 0,
            },
        ];
        assert_eq!(
            validate_opaque_pack_inventory(&entries),
            Err(PackValidationIssue::NotARegularFile {
                rel_path: "link".into()
            })
        );
    }

    #[test]
    fn opaque_validation_refuses_a_special_file_anywhere() {
        let entries = vec![
            file("a", 1),
            PackEntry {
                rel_path: "fifo".into(),
                kind: PackEntryKind::Other,
                size: 0,
            },
        ];
        assert_eq!(
            validate_opaque_pack_inventory(&entries),
            Err(PackValidationIssue::NotARegularFile {
                rel_path: "fifo".into()
            })
        );
    }

    #[test]
    fn opaque_validation_refuses_an_empty_directory_beside_files() {
        // A pack with at least one file AND an empty directory refuses:
        // the empty directory cannot round-trip through a files-only
        // manifest — never a silently altered tree.
        let entries = vec![file("payload", 8), dir("empty")];
        assert_eq!(
            validate_opaque_pack_inventory(&entries),
            Err(PackValidationIssue::EmptyDirectory {
                rel_path: "empty".into()
            })
        );
        // A directory holding ONLY an empty subdirectory is refused too
        // (no descendant file exists anywhere below it).
        let nested = vec![file("payload", 8), dir("a"), dir("a/b")];
        assert_eq!(
            validate_opaque_pack_inventory(&nested),
            Err(PackValidationIssue::EmptyDirectory {
                rel_path: "a".into()
            })
        );
        // A directory that DOES hold a file (directly or nested) passes.
        let populated = vec![file("payload", 8), dir("a"), dir("a/b"), file("a/b/x", 4)];
        assert!(validate_opaque_pack_inventory(&populated).is_ok());
    }

    #[test]
    fn opaque_validation_refuses_an_empty_pack() {
        assert_eq!(
            validate_opaque_pack_inventory(&[]),
            Err(PackValidationIssue::EmptyPack)
        );
        // Directories alone hold no acquirable bytes either.
        let dirs_only = vec![dir("data")];
        assert_eq!(
            validate_opaque_pack_inventory(&dirs_only),
            Err(PackValidationIssue::EmptyPack)
        );
    }

    #[test]
    fn opaque_validation_reuses_the_lunii_depth_ceiling() {
        // A file inside a 2-level tree sits exactly under the ceiling —
        // the same shape as the Lunii `rf/000/x`.
        let ok = vec![dir("a"), dir("a/b"), file("a/b/x", 4)];
        assert!(validate_opaque_pack_inventory(&ok).is_ok());
        // One level deeper refuses (directory first, like the walker).
        let deep_dir = vec![dir("a"), dir("a/b"), dir("a/b/c")];
        assert_eq!(
            validate_opaque_pack_inventory(&deep_dir),
            Err(PackValidationIssue::TooDeep {
                rel_path: "a/b/c".into()
            })
        );
        let deep_file = vec![file("a/b/c/x", 4)];
        assert_eq!(
            validate_opaque_pack_inventory(&deep_file),
            Err(PackValidationIssue::TooDeep {
                rel_path: "a/b/c/x".into()
            })
        );
    }

    #[test]
    fn opaque_validation_reuses_the_lunii_count_and_byte_bounds() {
        let too_many: Vec<PackEntry> = (0..=MAX_IMPORT_PACK_FILES)
            .map(|i| file(&format!("f{i}"), 1))
            .collect();
        assert!(matches!(
            validate_opaque_pack_inventory(&too_many),
            Err(PackValidationIssue::TooManyFiles { .. })
        ));

        let too_large = vec![
            file("a", MAX_IMPORT_PACK_BYTES),
            file("b", 1), // one byte over the total
        ];
        assert!(matches!(
            validate_opaque_pack_inventory(&too_large),
            Err(PackValidationIssue::TooLarge { .. })
        ));
    }

    #[test]
    fn empty_pack_issue_maps_to_pack_invalid_source() {
        assert_eq!(PackValidationIssue::EmptyPack.source_tag(), "pack_invalid");
        assert_eq!(
            PackValidationIssue::EmptyDirectory {
                rel_path: "empty".into()
            }
            .source_tag(),
            "pack_invalid"
        );
    }
}
