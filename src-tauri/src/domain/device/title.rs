//! Pack-title recognition model.
//!
//! A Lunii device stores only each pack's UUID — never its title (verified
//! against `marian-m12l/studio`: the on-device writer persists `ni/li/ri/si/
//! rf/sf/nm`, never the title). Recognizing a story therefore means looking
//! its UUID up in a LOCAL index `UUID → title`, exactly STUdio's model:
//!
//! - **User** titles: names the user typed for a pack (`set_device_story_title`).
//! - **Official** titles: the commercial catalog cached from Lunii once,
//!   on an explicit user action (STUdio's `official.json`).
//! - **Unofficial** titles: derived offline from Rustory's own library — the
//!   title of a local story already linked to that pack UUID (an imported or
//!   transferred story). This is what guarantees a story the user CREATED or
//!   imported is never shown as "non reconnue" (STUdio's `unofficial.json`).
//!
//! This module is framework-free: it carries the priority RESOLUTION rule
//! and the source taxonomy only. Gathering the candidates (SQLite reads,
//! the `story_imports → stories` join) lives in the application layer; the
//! catalog fetch lives in `infrastructure/`.
//!
//! Honesty invariant (UX): the winning title always carries its
//! [`PackTitleSource`] so the UI can show provenance and NEVER present a
//! user-typed or community title as "officiel".

/// Where a recognized title came from. Ordered by resolution priority:
/// a user-typed title outranks the official catalog, which outranks a
/// title inferred from the local library. The absence of any candidate is
/// modeled as `Option::None` at the resolution boundary — there is no
/// `Unknown` variant, because an unknown pack has no title to attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackTitleSource {
    /// A name the user typed for this pack (highest authority).
    User,
    /// The official Lunii commercial catalog, cached locally.
    Official,
    /// Inferred offline from a local story already linked to this pack
    /// (import provenance, or a community index in a later story).
    Unofficial,
}

impl PackTitleSource {
    /// Stable lowercase wire/storage token. Used both as the DTO
    /// `titleSource` value and as the `pack_metadata.source` column value
    /// so the persisted form and the wire form never drift.
    pub fn as_tag(self) -> &'static str {
        match self {
            PackTitleSource::User => "user",
            PackTitleSource::Official => "official",
            PackTitleSource::Unofficial => "unofficial",
        }
    }

    /// Parse a stored/wire tag back into a source. Returns `None` for an
    /// unknown token so a corrupt `pack_metadata.source` value degrades to
    /// "non reconnue" rather than panicking.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "user" => Some(PackTitleSource::User),
            "official" => Some(PackTitleSource::Official),
            "unofficial" => Some(PackTitleSource::Unofficial),
            _ => None,
        }
    }
}

/// A title value before it is attributed to a source — the raw text plus
/// an optional cover reference (an official catalog cover URL; user and
/// unofficial titles usually have none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleValue {
    pub title: String,
    pub thumbnail: Option<String>,
}

impl TitleValue {
    pub fn new(title: impl Into<String>, thumbnail: Option<String>) -> Self {
        Self {
            title: title.into(),
            thumbnail,
        }
    }
}

/// A recognized title together with its provenance. The resolution output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackTitle {
    pub title: String,
    pub source: PackTitleSource,
    pub thumbnail: Option<String>,
}

/// The per-origin candidates gathered for ONE pack UUID. Any slot may be
/// empty; the resolver picks the highest-priority non-empty one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackTitleCandidates {
    pub user: Option<TitleValue>,
    pub official: Option<TitleValue>,
    pub unofficial: Option<TitleValue>,
}

impl PackTitleCandidates {
    /// Resolve to the winning title by the fixed priority
    /// **User > Official > Unofficial**. `None` ⇒ no candidate at all ⇒ the
    /// pack stays genuinely "non reconnue". A user-typed title is therefore
    /// NEVER silently overwritten by a later official/unofficial match.
    pub fn resolve(&self) -> Option<PackTitle> {
        if let Some(v) = &self.user {
            return Some(PackTitle {
                title: v.title.clone(),
                source: PackTitleSource::User,
                thumbnail: v.thumbnail.clone(),
            });
        }
        if let Some(v) = &self.official {
            return Some(PackTitle {
                title: v.title.clone(),
                source: PackTitleSource::Official,
                thumbnail: v.thumbnail.clone(),
            });
        }
        if let Some(v) = &self.unofficial {
            return Some(PackTitle {
                title: v.title.clone(),
                source: PackTitleSource::Unofficial,
                thumbnail: v.thumbnail.clone(),
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(title: &str) -> TitleValue {
        TitleValue::new(title, None)
    }

    #[test]
    fn no_candidate_resolves_to_none_unknown_pack() {
        assert_eq!(PackTitleCandidates::default().resolve(), None);
    }

    #[test]
    fn user_title_outranks_official_and_unofficial() {
        let candidates = PackTitleCandidates {
            user: Some(value("Mon titre")),
            official: Some(value("Titre officiel")),
            unofficial: Some(value("Titre local")),
        };
        let resolved = candidates.resolve().expect("a title");
        assert_eq!(resolved.title, "Mon titre");
        assert_eq!(resolved.source, PackTitleSource::User);
    }

    #[test]
    fn official_outranks_unofficial_when_no_user_title() {
        let candidates = PackTitleCandidates {
            user: None,
            official: Some(value("Titre officiel")),
            unofficial: Some(value("Titre local")),
        };
        let resolved = candidates.resolve().expect("a title");
        assert_eq!(resolved.title, "Titre officiel");
        assert_eq!(resolved.source, PackTitleSource::Official);
    }

    #[test]
    fn unofficial_used_only_when_it_is_the_sole_candidate() {
        let candidates = PackTitleCandidates {
            user: None,
            official: None,
            unofficial: Some(value("Histoire importée")),
        };
        let resolved = candidates.resolve().expect("a title");
        assert_eq!(resolved.title, "Histoire importée");
        assert_eq!(resolved.source, PackTitleSource::Unofficial);
    }

    #[test]
    fn thumbnail_is_carried_through_resolution() {
        let candidates = PackTitleCandidates {
            official: Some(TitleValue::new("Titre", Some("cover.png".into()))),
            ..Default::default()
        };
        let resolved = candidates.resolve().expect("a title");
        assert_eq!(resolved.thumbnail.as_deref(), Some("cover.png"));
    }

    #[test]
    fn tag_round_trips_for_every_source() {
        for source in [
            PackTitleSource::User,
            PackTitleSource::Official,
            PackTitleSource::Unofficial,
        ] {
            assert_eq!(PackTitleSource::from_tag(source.as_tag()), Some(source));
        }
    }

    #[test]
    fn unknown_tag_does_not_resolve_to_a_source() {
        assert_eq!(PackTitleSource::from_tag("community-v9"), None);
        assert_eq!(PackTitleSource::from_tag(""), None);
    }

    #[test]
    fn tags_are_the_canonical_lowercase_tokens() {
        assert_eq!(PackTitleSource::User.as_tag(), "user");
        assert_eq!(PackTitleSource::Official.as_tag(), "official");
        assert_eq!(PackTitleSource::Unofficial.as_tag(), "unofficial");
    }
}
