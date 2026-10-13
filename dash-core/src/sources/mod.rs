pub mod alerts;
pub mod prom;
pub mod rektslug;

use url::Url;

use crate::config::Endpoint;
use crate::model::{Reading, Sheet};

/// Joins `path` onto the (trailing-slash normalised) base and appends query pairs.
pub(crate) fn endpoint_url(base: &Url, path: &str, params: &[(&str, &str)]) -> Url {
    // `path` is a compile-time literal, so the join cannot fail on a valid base.
    let mut url = base.join(path).unwrap_or_else(|_| base.clone());
    if !params.is_empty() {
        url.query_pairs_mut().extend_pairs(params);
    }
    url
}

/// Sheet for a source whose env var is unset or invalid: one missing row, never an error.
pub(crate) fn unavailable_sheet(
    id: &str,
    title: &str,
    row_id: &str,
    component: &str,
    ep: &Endpoint,
) -> Option<Sheet> {
    let reason = ep.unavailable_reason()?;
    let row = Reading::missing(component, &ep.label(), &reason).into_row(row_id);
    Some(Sheet::new(id, title, vec![row]))
}
