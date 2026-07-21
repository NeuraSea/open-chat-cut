use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, net::lookup_host, sync::watch};
use url::Url;

use crate::content_store::{HashedSource, create_private_file};

const MAX_REDIRECTS: usize = 5;
const SNIFF_BYTES: usize = 512;

#[derive(Clone, Copy)]
enum ResponsePolicy {
    Media,
    Html,
}

#[derive(Debug)]
pub struct RemoteDownload {
    pub temporary_path: PathBuf,
    pub final_url: Url,
    pub source_name: String,
    pub response_mime_type: Option<String>,
    pub hashed: HashedSource,
}

fn ipv4_is_public(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    match octets {
        [0, ..]
        | [10, ..]
        | [127, ..]
        | [169, 254, ..]
        | [192, 0, 0, ..]
        | [192, 0, 2, ..]
        | [192, 88, 99, ..]
        | [192, 168, ..]
        | [198, 18 | 19, ..]
        | [198, 51, 100, ..]
        | [203, 0, 113, ..]
        | [224..=255, ..] => false,
        [100, second, ..] if (64..=127).contains(&second) => false,
        [172, second, ..] if (16..=31).contains(&second) => false,
        _ => true,
    }
}

fn ipv6_is_public(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return ipv4_is_public(mapped);
    }
    let segments = address.segments();
    // Public global unicast currently occupies 2000::/3. Tunnel, benchmark,
    // documentation, and ORCHID ranges remain blocked even inside that space.
    if !(0x2000..=0x3fff).contains(&segments[0]) {
        return false;
    }
    if segments[0] == 0x2001 {
        if segments[1] == 0x0db8 || segments[1] == 0x0002 {
            return false;
        }
        if segments[1] == 0x0000
            || (segments[1] & 0xfff0) == 0x0010
            || (segments[1] & 0xfff0) == 0x0020
        {
            return false;
        }
    }
    segments[0] != 0x2002
}

pub fn ip_is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => ipv4_is_public(address),
        IpAddr::V6(address) => ipv6_is_public(address),
    }
}

pub fn validate_remote_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("remote media URL must use http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("remote media URL must not contain credentials");
    }
    if url.host_str().is_none() {
        bail!("remote media URL must contain a host");
    }
    if url.fragment().is_some() {
        bail!("remote media URL must not contain a fragment");
    }
    Ok(())
}

pub(crate) async fn pinned_http_client(
    url: &Url,
    allow_private_network: bool,
    timeout: Duration,
    user_agent: &str,
) -> Result<Client> {
    validate_remote_url(url)?;
    let host = url.host_str().context("remote URL has no host")?;
    let port = url
        .port_or_known_default()
        .context("remote URL has no usable port")?;
    let mut addresses = lookup_host((host, port))
        .await
        .with_context(|| format!("resolve remote media host {host}"))?
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        bail!("remote media host resolved to no addresses");
    }
    if !allow_private_network
        && let Some(blocked) = addresses.iter().find(|address| !ip_is_public(address.ip()))
    {
        bail!(
            "remote media host resolves to blocked address {}",
            blocked.ip()
        );
    }
    let addresses = addresses
        .into_iter()
        .map(|address| SocketAddr::new(address.ip(), port))
        .collect::<Vec<_>>();
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .user_agent(user_agent)
        .resolve_to_addrs(host, &addresses)
        .build()
        .context("build pinned remote media client")
}

async fn pinned_client(url: &Url) -> Result<Client> {
    pinned_http_client(
        url,
        false,
        Duration::from_secs(10 * 60),
        "OpenChatCut/0.1 remote-media-import",
    )
    .await
}

fn normalized_mime(value: &str) -> Option<String> {
    let value = value.split(';').next()?.trim().to_ascii_lowercase();
    (!value.is_empty()).then_some(value)
}

fn allowed_response_mime(value: Option<&str>, policy: ResponsePolicy) -> bool {
    let Some(value) = value.and_then(normalized_mime) else {
        return matches!(policy, ResponsePolicy::Media);
    };
    match policy {
        ResponsePolicy::Media => {
            value.starts_with("image/")
                || value.starts_with("audio/")
                || value.starts_with("video/")
                || matches!(
                    value.as_str(),
                    "application/octet-stream" | "application/ogg"
                )
        }
        ResponsePolicy::Html => matches!(value.as_str(), "text/html" | "application/xhtml+xml"),
    }
}

fn accept_header(policy: ResponsePolicy) -> &'static str {
    match policy {
        ResponsePolicy::Media => "video/*, audio/*, image/*, application/octet-stream",
        ResponsePolicy::Html => "text/html, application/xhtml+xml;q=0.9",
    }
}

fn source_name(url: &Url) -> String {
    url.path_segments()
        .and_then(Iterator::last)
        .filter(|value| !value.is_empty() && value.len() <= 255)
        .map(|value| value.replace(['/', '\\'], "_"))
        .unwrap_or_else(|| "remote-media".to_owned())
}

pub async fn download_public_media(
    requested_url: &str,
    expected_mime_type: Option<&str>,
    temporary_directory: &Path,
    maximum_bytes: u64,
) -> Result<RemoteDownload> {
    download_media(
        requested_url,
        expected_mime_type,
        temporary_directory,
        maximum_bytes,
        false,
        None,
        ResponsePolicy::Media,
    )
    .await
}

pub(crate) async fn download_public_html_cancellable(
    requested_url: &str,
    temporary_directory: &Path,
    maximum_bytes: u64,
    cancellation: watch::Receiver<bool>,
) -> Result<RemoteDownload> {
    download_media(
        requested_url,
        None,
        temporary_directory,
        maximum_bytes,
        false,
        Some(cancellation),
        ResponsePolicy::Html,
    )
    .await
}

pub(crate) async fn download_media_with_policy_cancellable(
    requested_url: &str,
    expected_mime_type: Option<&str>,
    temporary_directory: &Path,
    maximum_bytes: u64,
    allow_private_network: bool,
    cancellation: watch::Receiver<bool>,
) -> Result<RemoteDownload> {
    download_media(
        requested_url,
        expected_mime_type,
        temporary_directory,
        maximum_bytes,
        allow_private_network,
        Some(cancellation),
        ResponsePolicy::Media,
    )
    .await
}

async fn download_media(
    requested_url: &str,
    expected_mime_type: Option<&str>,
    temporary_directory: &Path,
    maximum_bytes: u64,
    allow_private_network: bool,
    mut cancellation: Option<watch::Receiver<bool>>,
    response_policy: ResponsePolicy,
) -> Result<RemoteDownload> {
    let mut url = Url::parse(requested_url).context("parse remote media URL")?;
    validate_remote_url(&url)?;
    let expected_mime_type = expected_mime_type
        .map(|value| {
            normalized_mime(value).context("expectedMimeType must be a non-empty MIME type")
        })
        .transpose()?;
    let mut visited = HashSet::new();
    let response = loop {
        if !visited.insert(url.as_str().to_owned()) {
            bail!("remote media redirect loop detected");
        }
        let client = if allow_private_network {
            pinned_http_client(
                &url,
                true,
                Duration::from_secs(10 * 60),
                "OpenChatCut/0.1 provider-output",
            )
            .await?
        } else {
            pinned_client(&url).await?
        };
        let request = client
            .get(url.clone())
            .header(header::ACCEPT, accept_header(response_policy));
        let response = if let Some(receiver) = cancellation.as_mut() {
            tokio::select! {
                changed = receiver.changed() => {
                    let _ = changed;
                    bail!("remote media download cancelled");
                }
                response = request.send() => response,
            }
        } else {
            request.send().await
        }
        .with_context(|| format!("download remote media from {url}"))?;
        if response.status().is_redirection() {
            if visited.len() > MAX_REDIRECTS {
                bail!("remote media exceeded the redirect limit");
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .context("remote media redirect has no valid Location")?;
            url = url
                .join(location)
                .context("resolve remote media redirect")?;
            validate_remote_url(&url)?;
            continue;
        }
        if response.status() != StatusCode::OK {
            bail!("remote media server returned HTTP {}", response.status());
        }
        break response;
    };
    if let Some(length) = response.content_length()
        && length > maximum_bytes
    {
        bail!("remote media Content-Length exceeds the byte limit");
    }
    let response_mime_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalized_mime);
    if !allowed_response_mime(response_mime_type.as_deref(), response_policy) {
        bail!(match response_policy {
            ResponsePolicy::Media => "remote response MIME type is not media",
            ResponsePolicy::Html => "remote response MIME type is not HTML",
        });
    }
    if let (Some(expected), Some(actual)) = (&expected_mime_type, &response_mime_type)
        && actual != "application/octet-stream"
        && expected != actual
    {
        bail!("remote response MIME type does not match expectedMimeType");
    }

    let temporary_path =
        temporary_directory.join(format!(".remote-import-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = create_private_file(&temporary_path).await?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut prefix = Vec::with_capacity(SNIFF_BYTES);
    let receive = async {
        loop {
            let next = if let Some(receiver) = cancellation.as_mut() {
                tokio::select! {
                    changed = receiver.changed() => {
                        let _ = changed;
                        bail!("remote media download cancelled");
                    }
                    next = stream.next() => next,
                }
            } else {
                stream.next().await
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.context("read remote media response")?;
            total = total
                .checked_add(chunk.len() as u64)
                .context("remote media size overflow")?;
            if total > maximum_bytes {
                bail!("remote media grew beyond the byte limit");
            }
            let remaining = SNIFF_BYTES.saturating_sub(prefix.len());
            prefix.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            hasher.update(&chunk);
            output.write_all(&chunk).await?;
        }
        if total == 0 {
            bail!("remote media response is empty");
        }
        output.flush().await?;
        output.sync_all().await?;
        Ok(())
    }
    .await;
    drop(output);
    if let Err(error) = receive {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        return Err(error);
    }
    Ok(RemoteDownload {
        temporary_path,
        final_url: url.clone(),
        source_name: source_name(&url),
        response_mime_type,
        hashed: HashedSource {
            sha256: hex::encode(hasher.finalize()),
            size: total,
            prefix,
        },
    })
}

pub fn is_blocked_network_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("blocked address")
        || message.contains("must use http or https")
        || message.contains("must not contain credentials")
        || message.contains("redirect")
}

pub fn is_size_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("byte limit") || message.contains("Content-Length")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_every_common_ssrf_address_class() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!ip_is_public(address.parse().unwrap()), "{address}");
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(ip_is_public(address.parse().unwrap()), "{address}");
        }
    }

    #[test]
    fn rejects_credentials_and_non_http_schemes() {
        assert!(validate_remote_url(&Url::parse("file:///etc/passwd").unwrap()).is_err());
        assert!(
            validate_remote_url(&Url::parse("http://user:pass@example.com/a.mp4").unwrap())
                .is_err()
        );
    }

    #[test]
    fn html_downloads_require_an_explicit_html_mime_type() {
        assert!(allowed_response_mime(
            Some("text/html; charset=utf-8"),
            ResponsePolicy::Html
        ));
        assert!(allowed_response_mime(
            Some("application/xhtml+xml"),
            ResponsePolicy::Html
        ));
        assert!(!allowed_response_mime(None, ResponsePolicy::Html));
        assert!(!allowed_response_mime(
            Some("application/octet-stream"),
            ResponsePolicy::Html
        ));
    }
}
