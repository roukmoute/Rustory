//! Packaging contract of the file association (`File Association
//! Contract`): locks `tauri.conf.json` and the Linux bundle files
//! COHERENT WITH EACH OTHER. The verify pipeline never bundles — this
//! guard is the only net against a silent packaging regression (a
//! renamed key, a deleted template, a drifted MIME type would
//! otherwise break the association without failing anything).
//! Hermetic: reads the repository files only — zero network, zero
//! bundler, zero OS mutation.

use std::path::{Path, PathBuf};

use rustory_lib::domain::export::RUSTORY_ARTIFACT_EXTENSION;
use rustory_lib::domain::import::LINUX_PACKAGE_MIME_XML;

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn tauri_conf() -> serde_json::Value {
    let raw =
        std::fs::read_to_string(manifest_path("tauri.conf.json")).expect("read tauri.conf.json");
    serde_json::from_str(&raw).expect("parse tauri.conf.json")
}

#[test]
fn the_bundle_declares_exactly_one_rustory_file_association() {
    let conf = tauri_conf();
    let associations = conf["bundle"]["fileAssociations"]
        .as_array()
        .expect("bundle.fileAssociations declared");
    assert_eq!(
        associations.len(),
        1,
        "the .rustory artifact is the ONLY associated type"
    );
    let association = &associations[0];
    // The extension follows the domain's single truth (dot-less, the
    // save-dialog convention the bundler shares).
    assert_eq!(
        association["ext"],
        serde_json::json!([RUSTORY_ARTIFACT_EXTENSION])
    );
    // ProgId Windows (no space) AND macOS CFBundleTypeName.
    assert_eq!(association["name"], "RustoryStory");
    // Windows-visible (Explorer Type column, "Open with" dialog).
    assert_eq!(association["description"], "Histoire Rustory");
    // Linux MimeType= / macOS UTI tag — must stay byte-identical to
    // the shared-mime-info XML (locked below).
    assert_eq!(association["mimeType"], "application/x-rustory");
    // Exact serde casing of the closed enums (deny_unknown_fields:
    // a drifted casing breaks the build, this locks the intent).
    assert_eq!(association["role"], "Editor");
    assert_eq!(association["rank"], "Owner");
    // The exported type declaration macOS requires for a proprietary
    // type; a `.rustory` v1 IS a UTF-8 JSON file.
    assert_eq!(
        association["exportedType"]["identifier"],
        "fr.roukmoute.rustory.story"
    );
    assert_eq!(
        association["exportedType"]["conformsTo"],
        serde_json::json!(["public.json"])
    );
    // The exported identifier extends the app identifier — one
    // reverse-DNS family, never a parallel namespace.
    let app_identifier = conf["identifier"].as_str().expect("app identifier");
    assert_eq!(
        association["exportedType"]["identifier"]
            .as_str()
            .expect("exported identifier"),
        format!("{app_identifier}.story")
    );
}

#[test]
fn both_linux_desktop_templates_exist_and_carry_the_field_code_and_mime_type() {
    let conf = tauri_conf();
    let deb_template = conf["bundle"]["linux"]["deb"]["desktopTemplate"]
        .as_str()
        .expect("deb.desktopTemplate declared");
    let rpm_template = conf["bundle"]["linux"]["rpm"]["desktopTemplate"]
        .as_str()
        .expect("rpm.desktopTemplate declared");
    // One template serves both packages — a fork would drift.
    assert_eq!(deb_template, rpm_template);
    let template_path = manifest_path(deb_template);
    assert!(
        template_path.is_file(),
        "the declared desktop template must exist at {template_path:?}"
    );
    let template = std::fs::read_to_string(&template_path).expect("read desktop template");
    // THE pitfall this file exists for: the CLI's embedded template
    // has no field code — without `%F` the launcher never passes the
    // opened file to the app.
    assert!(
        template.contains("Exec={{exec}} %F"),
        "the desktop template must pass the opened files (`Exec={{{{exec}}}} %F`)"
    );
    // The MimeType= line wires the desktop entry to the declared MIME
    // type (the bundler fills the handlebars variable from
    // `fileAssociations.mimeType`).
    assert!(
        template.contains("MimeType={{mime_type}}"),
        "the desktop template must declare the MimeType= line"
    );
    // The skeleton stays the embedded one: the window-class hint the
    // default template carries must survive the customization.
    assert!(template.contains("StartupWMClass={{exec}}"));
}

#[test]
fn both_linux_packages_ship_the_shared_mime_info_xml() {
    let conf = tauri_conf();
    // The packaging target IS the witness path the install probe's
    // frontier checks — one constant, two locked sides.
    let target = LINUX_PACKAGE_MIME_XML;
    let deb_source = conf["bundle"]["linux"]["deb"]["files"][target]
        .as_str()
        .expect("deb.files ships the shared-mime-info XML");
    let rpm_source = conf["bundle"]["linux"]["rpm"]["files"][target]
        .as_str()
        .expect("rpm.files ships the shared-mime-info XML");
    // One source XML serves both packages.
    assert_eq!(deb_source, rpm_source);
    let source_path = manifest_path(deb_source);
    assert!(
        source_path.is_file(),
        "the declared XML source must exist at {source_path:?}"
    );
    let xml = std::fs::read_to_string(&source_path).expect("read mime xml");
    // THE second pitfall: the bundler generates no shared-mime-info —
    // without this XML Linux never recognizes the extension. Its MIME
    // type must stay byte-identical to the fileAssociations entry.
    assert!(xml.contains(r#"type="application/x-rustory""#));
    // The glob binds the extension to the type; the sub-class keeps
    // generic JSON tooling able to open a `.rustory` (the artifact IS
    // a UTF-8 JSON file — the documented format contract).
    assert!(xml.contains(r#"glob pattern="*.rustory""#));
    assert!(xml.contains(r#"sub-class-of type="application/json""#));
}

#[test]
fn the_appimage_appdir_embeds_the_same_shared_mime_info_xml() {
    // An AppImage registers nothing by default — but its frozen
    // reason promises that an integration tool (or a manual desktop
    // entry) CAN add the association. That promise is only operative
    // if the AppDir actually carries the shared-mime-info XML those
    // tools would register: same target, same source as the system
    // packages.
    let conf = tauri_conf();
    let appimage_source = conf["bundle"]["linux"]["appimage"]["files"][LINUX_PACKAGE_MIME_XML]
        .as_str()
        .expect("appimage.files embeds the shared-mime-info XML");
    let deb_source = conf["bundle"]["linux"]["deb"]["files"][LINUX_PACKAGE_MIME_XML]
        .as_str()
        .expect("deb.files ships the shared-mime-info XML");
    assert_eq!(
        appimage_source, deb_source,
        "one source XML serves every Linux channel"
    );
    assert!(manifest_path(appimage_source).is_file());
}
