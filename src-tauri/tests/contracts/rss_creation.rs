//! Wire contracts of the RSS external-source creation flow: the frozen FR
//! copies (byte-for-byte), the new DTO shapes, the transport error code —
//! and the invariance of the sibling flows' literals (the `.rustory` and
//! folder copies must not move).

use rustory_lib::application::import_export::rss_creation;
use rustory_lib::domain::import::{
    parse_rss, rss_item_fingerprint, RecognitionFinding, RssItemRef,
};
use rustory_lib::infrastructure::device::rss_source;
use rustory_lib::ipc::dto::import_export::{
    finding_message, rss_finding_message, rss_import_findings_from_summary,
    serialize_findings_summary, structured_folder_finding_message,
};
use rustory_lib::ipc::dto::{
    ImportAspectDto, ImportCategoryDto, RssCreationOutcomeDto, RssItemRefDto, RssPreviewDto,
    StoryCardDto,
};

// ===== Frozen copies (product-language.md — byte-for-byte) =====

#[test]
fn transport_failure_copy_is_frozen() {
    let err = rss_source::fetch_error("request");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "RSS_SOURCE_UNREACHABLE");
    assert_eq!(
        v["message"],
        "Récupération du flux impossible: la source est injoignable."
    );
    assert_eq!(
        v["userAction"],
        "Vérifie l'adresse du flux et ta connexion, puis réessaie."
    );
    assert_eq!(v["details"]["source"], "network");
    assert_eq!(v["details"]["stage"], "request");
}

#[test]
fn invalid_address_copy_is_frozen() {
    let err = rss_creation::invalid_feed_url_error();
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "RSS_SOURCE_UNREACHABLE");
    assert_eq!(
        v["message"],
        "Récupération du flux impossible: l'adresse du flux n'est pas valide."
    );
    assert_eq!(
        v["userAction"],
        "Saisis une adresse http(s) complète puis réessaie."
    );
    assert_eq!(v["details"]["stage"], "url_invalid");
}

#[test]
fn the_four_verdict_copies_are_frozen_with_their_gesture() {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    assert_eq!(
        rss_finding_message(A::Envelope, C::Blocking),
        "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux."
    );
    assert_eq!(
        rss_finding_message(A::FormatVersion, C::Blocking),
        "Ce flux n'est pas au format RSS supporté. Relance la récupération du flux."
    );
    assert_eq!(
        rss_finding_message(A::Structure, C::Blocking),
        "Ce flux ne contient aucun épisode exploitable. Relance la récupération du flux."
    );
    // The fourth verdict (`La source a changé depuis la récupération.`) is
    // the accept-time refusal — a tagged outcome discriminant rendered by
    // the surface, not a finding pair; its copy is frozen in the TS test.
}

#[test]
fn the_rss_attention_pair_copies_are_frozen() {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    assert_eq!(
        rss_finding_message(A::Source, C::Ambiguous),
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser."
    );
    assert_eq!(
        rss_finding_message(A::Title, C::Ambiguous),
        "Le titre de l'épisode était absent ou a été ajusté à l'ingestion. Vérifie le titre de l'histoire dans l'éditeur."
    );
    assert_eq!(
        rss_finding_message(A::Structure, C::Ambiguous),
        "Le texte de l'épisode était absent ou a été ajusté à l'ingestion (balises HTML retirées, blancs ou longueur réduits). Relis le texte dans l'éditeur."
    );
    assert_eq!(
        rss_finding_message(A::Media, C::Missing),
        "Le média distant référencé par la source n'a pas été récupéré. Ajoute le média manuellement dans l'éditeur."
    );
    assert_eq!(
        rss_finding_message(A::Envelope, C::Recognized),
        "Le flux RSS est lisible."
    );
    assert_eq!(
        rss_finding_message(A::FormatVersion, C::Recognized),
        "Le flux est au format RSS 2.0 supporté."
    );
    assert_eq!(
        rss_finding_message(A::Title, C::Recognized),
        "Le titre de l'épisode est valide."
    );
    assert_eq!(
        rss_finding_message(A::Structure, C::Recognized),
        "Le texte de l'épisode est reconnu."
    );
}

// ===== Invariance of the sibling flows' literals =====

#[test]
fn the_rustory_and_folder_copies_stay_verbatim() {
    use ImportAspectDto as A;
    use ImportCategoryDto as C;
    // A spot-check per flow on the pairs the RSS flow overloads: the
    // shared table and the folder table must not have moved.
    assert_eq!(
        finding_message(A::Envelope, C::Blocking),
        "Le fichier n'est pas un artefact Rustory valide."
    );
    assert_eq!(
        finding_message(A::Media, C::Missing),
        "Certains fichiers audio ou image référencés par le dossier sont introuvables. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur."
    );
    assert_eq!(
        structured_folder_finding_message(A::Envelope, C::Blocking),
        "Le dossier ne contient pas de manifest histoire.json lisible. Corrige le dossier puis relance l'analyse."
    );
    assert_eq!(
        structured_folder_finding_message(A::Structure, C::Blocking),
        "La structure du manifest est incomplète ou incohérente. Corrige le manifest puis relance l'analyse."
    );
    // The `source` defensive copies exist in every table (exhaustive
    // match) without stealing the living RSS wording.
    assert_eq!(
        finding_message(A::Source, C::Ambiguous),
        rss_finding_message(A::Source, C::Ambiguous)
    );
}

// ===== New DTO wire shapes =====

fn exploitable_preview() -> RssPreviewDto {
    let analysis = parse_rss(
        "<rss version=\"2.0\"><channel><title>Flux</title>\
         <item><title>Episode</title><description>Texte.</description><guid>g-1</guid></item>\
         </channel></rss>"
            .as_bytes(),
    );
    RssPreviewDto::from_analysis("exemple.fr".into(), &analysis)
}

#[test]
fn rss_preview_wire_shape_round_trips_the_documented_contract() {
    let v = serde_json::to_value(exploitable_preview()).expect("ser");
    assert_eq!(v["sourceHost"], "exemple.fr");
    assert_eq!(v["blocked"], false);
    assert_eq!(v["state"], "needsReview");
    let item = &v["items"][0];
    assert_eq!(item["title"], "Episode");
    assert_eq!(item["summary"], "Texte.");
    assert_eq!(item["hasEnclosure"], false);
    // The reference carries the selector AND the previewed-content proof
    // (the fingerprint the accept re-verifies on the fresh fetch).
    assert_eq!(item["itemRef"]["kind"], "guid");
    assert_eq!(item["itemRef"]["guid"], "g-1");
    let wire_fingerprint = item["itemRef"]["fingerprint"]
        .as_str()
        .expect("fingerprint");
    let analysis = parse_rss(
        "<rss version=\"2.0\"><channel><title>Flux</title>\
         <item><title>Episode</title><description>Texte.</description><guid>g-1</guid></item>\
         </channel></rss>"
            .as_bytes(),
    );
    assert_eq!(
        wire_fingerprint,
        rss_item_fingerprint(&analysis.items[0]),
        "the wire proof is the canonical item fingerprint"
    );
    // Findings carry the RSS copy; the nominal source ambiguity is there.
    let findings = v["findings"].as_array().expect("findings");
    assert!(findings
        .iter()
        .any(|f| f["aspect"] == "source" && f["category"] == "ambiguous"));
    for snake in ["source_host", "item_ref", "has_enclosure"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn rss_item_ref_input_rejects_snake_case_unknown_fields_and_foreign_kinds() {
    let ok: RssItemRefDto = serde_json::from_value(serde_json::json!({
        "kind": "titleLink", "title": "Episode", "link": null, "fingerprint": "a".repeat(64),
    }))
    .expect("deser");
    assert_eq!(
        ok.to_domain(),
        RssItemRef::TitleLink {
            title: "Episode".into(),
            link: None
        }
    );
    assert_eq!(ok.fingerprint(), "a".repeat(64));
    assert!(serde_json::from_value::<RssItemRefDto>(serde_json::json!({
        "kind": "titleLink", "title": "E", "link": null, "fingerprint": "a".repeat(64), "extra": 1,
    }))
    .is_err());
    assert!(
        serde_json::from_value::<RssItemRefDto>(
            serde_json::json!({ "kind": "guid", "fingerprint": "a".repeat(64) })
        )
        .is_err(),
        "a guid reference without its guid must be refused"
    );
    assert!(
        serde_json::from_value::<RssItemRefDto>(serde_json::json!({ "kind": "guid", "guid": "g" }))
            .is_err(),
        "a reference without the previewed-content proof must be refused"
    );
    assert!(serde_json::from_value::<RssItemRefDto>(serde_json::json!({
        "kind": "index", "index": 3,
    }))
    .is_err());
}

#[test]
fn rss_creation_outcome_wire_shapes_are_frozen() {
    let created = RssCreationOutcomeDto::Created {
        story: StoryCardDto {
            id: "0197a5d0-0000-7000-8000-000000000000".into(),
            title: "Episode".into(),
            import_state: Some(rustory_lib::ipc::dto::ImportStateDto::NeedsReview),
            import_report: None,
        },
        report: Vec::new(),
    };
    let v = serde_json::to_value(&created).expect("ser");
    assert_eq!(v["kind"], "created");
    assert_eq!(v["story"]["importState"], "needsReview");
    assert!(v["report"].as_array().expect("report").is_empty());

    let changed = serde_json::to_value(RssCreationOutcomeDto::SourceChanged).expect("ser");
    assert_eq!(changed, serde_json::json!({ "kind": "sourceChanged" }));
}

#[test]
fn a_persisted_rss_summary_re_renders_through_the_rss_copy() {
    // The durable card report of an `rss` story is re-rendered from the
    // stored `(aspect, category)` pairs with the FEED wording.
    let findings = [
        RecognitionFinding::recognized(rustory_lib::domain::import::RecognitionAspect::Envelope),
        RecognitionFinding::ambiguous(rustory_lib::domain::import::RecognitionAspect::Source),
    ];
    let summary = serialize_findings_summary(&findings).expect("summary");
    let report = rss_import_findings_from_summary(&summary);
    assert_eq!(report.len(), 2);
    assert_eq!(report[0].message, "Le flux RSS est lisible.");
    assert!(report[1]
        .message
        .starts_with("Contenu ingéré depuis une source externe (RSS)."));
}
