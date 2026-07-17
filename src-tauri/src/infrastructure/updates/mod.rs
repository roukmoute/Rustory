//! Update infrastructure (`Update Availability Contract`): the bounded
//! consultation of the latest published official release. The home the
//! architecture map reserves for everything update-related — today the
//! read-only availability source, one day the update gesture's machinery
//! (a separate capability, deliberately absent).

pub mod release_source;

pub use release_source::{
    GithubHttpReleaseSource, UpdateFetchStage, UpdateReleaseSource, GITHUB_LATEST_RELEASE_ENDPOINT,
    MAX_UPDATE_RESPONSE_BYTES, UPDATE_CHECK_ENDPOINT_ENV,
};
