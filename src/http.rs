//! Bounded response reading shared by `api` and `site`.

use crate::{Error, Result};

/// Largest response body either client will read. Real pages and result
/// lists are well under a megabyte; anything bigger is a fault or an
/// attack, and is reported as [`Error::Parse`] instead of being buffered.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Status plus body, read in chunks up to [`MAX_RESPONSE_BYTES`]. A body
/// that cannot be read is an error, never an empty success.
pub(crate) async fn read_body(
    mut resp: reqwest::Response,
) -> Result<(reqwest::StatusCode, String)> {
    let status = resp.status();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(Error::Parse(format!(
                "response body too large (more than {} bytes)",
                MAX_RESPONSE_BYTES
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok((status, String::from_utf8_lossy(&buf).into_owned()))
}
