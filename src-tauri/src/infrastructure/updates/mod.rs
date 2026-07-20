//! Update infrastructure — the two machineries the architecture map
//! reserves this home for: the bounded read-only consultation of the
//! latest published release (`Update Availability Contract`) and the
//! updater-plugin gateway of the user-triggered gesture (`Update Apply
//! Contract`). Two sealed regimes: the information source never mutates,
//! the apply gateway alone touches the installation — behind its trait,
//! after the pure plan decision allowed it.

pub mod apply_gateway;
pub mod release_source;

pub use apply_gateway::{
    download_percent, map_check_error, map_download_error, TauriUpdaterGateway, UpdateApplyGateway,
    UpdateApplyProgressTick, UPDATE_FEED_CHECK_BUDGET, UPDATE_FEED_ENDPOINT,
    UPDATE_FEED_ENDPOINT_ENV,
};
pub use release_source::{
    GithubHttpReleaseSource, UpdateFetchStage, UpdateReleaseSource, GITHUB_LATEST_RELEASE_ENDPOINT,
    MAX_UPDATE_RESPONSE_BYTES, UPDATE_CHECK_ENDPOINT_ENV,
};
