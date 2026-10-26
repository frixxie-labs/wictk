use anyhow::{Context, Result};
use reqwest::{Client, Url};
use serde::{de::DeserializeOwned, Serialize};

/// Resolves `path` relative to `base`, whether or not `base` ends with a slash.
pub fn endpoint(base: &Url, path: &str) -> Result<Url> {
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(path)
        .with_context(|| format!("Invalid endpoint path '{path}' for {base}"))
}

pub async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: Url,
    query: &[(&str, &str)],
) -> Result<T> {
    client
        .get(url.clone())
        .query(query)
        .send()
        .await
        .with_context(|| format!("Request to {url} failed"))?
        .error_for_status()
        .with_context(|| format!("{url} returned an error"))?
        .json()
        .await
        .with_context(|| format!("Failed to decode response from {url}"))
}

pub async fn post_json<B: Serialize + ?Sized>(client: &Client, url: Url, body: &B) -> Result<()> {
    client
        .post(url.clone())
        .json(body)
        .send()
        .await
        .with_context(|| format!("Request to {url} failed"))?
        .error_for_status()
        .with_context(|| format!("{url} rejected the request"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_works_with_and_without_trailing_slash() {
        let cases = [
            ("http://host/", "http://host/api/x"),
            ("http://host", "http://host/api/x"),
            ("http://host/prefix", "http://host/prefix/api/x"),
            ("http://host/prefix/", "http://host/prefix/api/x"),
        ];

        for (base, expected) in cases {
            let url = endpoint(&base.parse().unwrap(), "api/x").unwrap();
            assert_eq!(url.as_str(), expected, "{base}");
        }
    }
}
