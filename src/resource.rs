use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub(crate) struct ResourceCache {
    bytes: HashMap<PathBuf, Vec<u8>>,
}

impl ResourceCache {
    pub(crate) async fn preload(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut cache = Self::default();
        let mut seen = HashSet::new();
        for path in paths {
            if !seen.insert(path.clone()) {
                continue;
            }
            match read_bytes(&path).await {
                Ok(bytes) => {
                    cache.bytes.insert(path, bytes);
                }
                Err(error) => {
                    log::debug!("failed to preload resource {}: {}", path.display(), error);
                }
            }
        }
        cache
    }

    pub(crate) fn get(&self, path: &Path) -> Option<&[u8]> {
        self.bytes.get(path).map(Vec::as_slice)
    }
}

pub(crate) async fn read_to_string(location: &Path) -> crate::Result<String> {
    let bytes = read_bytes(location).await?;
    String::from_utf8(bytes).map_err(|error| {
        crate::Error::InvalidInput(format!(
            "resource {} is not UTF-8: {error}",
            location.display()
        ))
    })
}

pub(crate) async fn read_bytes(location: &Path) -> crate::Result<Vec<u8>> {
    let value = location.to_string_lossy();
    if is_http_url(&value) {
        log::trace!("fetching HTTP resource {}", location.display());
        let response = reqwest::get(value.as_ref()).await.map_err(|error| {
            crate::Error::InvalidInput(format!("failed to fetch {}: {error}", location.display()))
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(crate::Error::InvalidInput(format!(
                "HTTP fetch for {} returned {status}",
                location.display()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "failed to read response body for {}: {error}",
                    location.display()
                ))
            })?;
        log::trace!(
            "fetched HTTP resource {} ({} byte(s))",
            location.display(),
            bytes.len()
        );
        return Ok(bytes);
    }
    log::trace!("reading filesystem resource {}", location.display());
    let bytes = tokio::fs::read(location).await?;
    log::trace!(
        "read filesystem resource {} ({} byte(s))",
        location.display(),
        bytes.len()
    );
    Ok(bytes)
}

/// Converts a local `file://` URL into a filesystem path.
///
/// HTML links and CSS `url()` references are URL-valued inputs. This helper
/// implements the local-file subset used by file-backed documents, following
/// the URL Standard's special `file` scheme parsing model:
/// <https://url.spec.whatwg.org/#file-scheme>.
pub fn file_url_to_path(value: &str) -> Option<PathBuf> {
    let rest = value.strip_prefix("file://")?;
    let path = if let Some(path) = rest.strip_prefix("localhost/") {
        format!("/{path}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };
    percent_decode_path(&path).map(PathBuf::from)
}

pub(crate) fn resolve_url_path(
    value: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Option<PathBuf> {
    log::trace!(
        "resolving resource URL value={value:?} base_url={} root_url={}",
        display_optional_path(base_url),
        display_optional_path(root_url)
    );
    if is_http_url(value) {
        let resolved = PathBuf::from(value);
        log::trace!(
            "resolved resource URL value={value:?} to {}",
            resolved.display()
        );
        return Some(resolved);
    }
    if let Some(base_url) = base_url.and_then(http_url_path)
        && let Some(network_path) = value.strip_prefix("//")
    {
        let scheme = base_url.split_once("://")?.0;
        let resolved = PathBuf::from(format!("{scheme}://{network_path}"));
        log::trace!(
            "resolved resource URL value={value:?} to {}",
            resolved.display()
        );
        return Some(resolved);
    }
    if let Some(path) = file_url_to_path(value) {
        log::trace!(
            "resolved resource URL value={value:?} to {}",
            path.display()
        );
        return Some(path);
    }
    if value.contains(':') || value.starts_with("//") {
        log::trace!("could not resolve unsupported resource URL value={value:?}");
        return None;
    }
    if let Some(root_url) = root_url.and_then(http_url_path)
        && let Some(relative) = value.strip_prefix('/')
    {
        let resolved = PathBuf::from(join_http_url(root_url, relative)?);
        log::trace!(
            "resolved resource URL value={value:?} to {}",
            resolved.display()
        );
        return Some(resolved);
    }
    if let Some(base_url) = base_url.and_then(http_url_path)
        && !Path::new(value).is_absolute()
    {
        let resolved = PathBuf::from(join_http_url(base_url, value)?);
        log::trace!(
            "resolved resource URL value={value:?} to {}",
            resolved.display()
        );
        return Some(resolved);
    }
    let path = Path::new(value);
    let resolved = if path.is_absolute() {
        if let Some(root_url) = root_url {
            value
                .strip_prefix('/')
                .map(|relative| root_url.join(relative))
                .or_else(|| Some(path.to_path_buf()))
        } else {
            Some(path.to_path_buf())
        }
    } else {
        base_url.map(|base_url| base_url.join(path))
    };
    match &resolved {
        Some(path) => log::trace!(
            "resolved resource URL value={value:?} to {}",
            path.display()
        ),
        None => log::trace!("could not resolve resource URL value={value:?}"),
    }
    resolved
}

fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

pub(crate) fn resource_parent(location: &Path) -> Option<PathBuf> {
    let value = location.to_string_lossy();
    if is_http_url(&value) {
        return http_parent_url(&value).map(PathBuf::from);
    }
    location.parent().map(Path::to_path_buf)
}

pub(crate) fn css_url_paths(
    source: &str,
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Vec<PathBuf> {
    css_urls(source)
        .into_iter()
        .filter_map(|url| resolve_url_path(&url, base_url, root_url))
        .collect()
}

fn css_urls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut urls = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if !bytes[index..]
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"url("))
        {
            index += 1;
            continue;
        }
        index += 4;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let Some(url) = (if matches!(bytes.get(index), Some(b'"' | b'\'')) {
            let quote = bytes[index];
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value = source.get(start..index).map(str::to_string);
            if bytes.get(index) == Some(&quote) {
                index += 1;
            }
            value
        } else {
            let start = index;
            while index < bytes.len() && bytes[index] != b')' {
                index += 1;
            }
            source
                .get(start..index)
                .map(|value| value.trim().to_string())
        }) else {
            break;
        };
        while bytes.get(index).is_some_and(|byte| *byte != b')') {
            index += 1;
        }
        if bytes.get(index) == Some(&b')') {
            index += 1;
        }
        if !url.is_empty() {
            urls.push(url);
        }
    }
    urls
}

fn percent_decode_path(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_value)?;
            let low = bytes.get(index + 2).copied().and_then(hex_value)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://")
}

fn http_url_path(path: &Path) -> Option<&str> {
    let value = path.to_str()?;
    is_http_url(value).then_some(value)
}

fn http_parent_url(value: &str) -> Option<String> {
    let (origin, path) = split_http_origin(value)?;
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    if parent.is_empty() {
        Some(origin.to_string())
    } else {
        Some(format!("{origin}{parent}"))
    }
}

fn join_http_url(base: &str, relative: &str) -> Option<String> {
    let (origin, base_path) = split_http_origin(base)?;
    let relative = relative.split_once('#').map_or(relative, |(path, _)| path);
    let relative = relative.split_once('?').map_or(relative, |(path, _)| path);
    let mut segments = base_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            value => segments.push(value.to_string()),
        }
    }
    if segments.is_empty() {
        Some(origin.to_string())
    } else {
        Some(format!("{origin}/{}", segments.join("/")))
    }
}

fn split_http_origin(value: &str) -> Option<(&str, &str)> {
    let rest = value.strip_prefix("http://")?;
    if let Some(index) = rest.find('/') {
        let slash = "http://".len() + index;
        Some((&value[..slash], &value[slash..]))
    } else {
        Some((value, ""))
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn converts_local_file_urls_to_paths() {
        assert_eq!(
            file_url_to_path("file:///Users/lee/Some%20File.html").unwrap(),
            PathBuf::from("/Users/lee/Some File.html")
        );
        assert_eq!(
            file_url_to_path("file://localhost/Users/lee/a.html").unwrap(),
            PathBuf::from("/Users/lee/a.html")
        );
        assert!(file_url_to_path("file://example.com/Users/lee/a.html").is_none());
    }

    #[tokio::test]
    async fn extracts_stylesheet_url_paths_from_rule_blocks() {
        let base_url = Path::new("/tmp/document/css");
        let root_url = Path::new("/tmp/wpt");
        let paths = css_url_paths(
            "body { background: url('../img/bg.png') } @font-face { src: url('/fonts/ahem.ttf') }",
            Some(base_url),
            Some(root_url),
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/tmp/document/css/../img/bg.png"),
                PathBuf::from("/tmp/wpt/fonts/ahem.ttf")
            ]
        );
    }

    #[tokio::test]
    async fn resolves_http_relative_and_root_relative_urls() {
        assert_eq!(
            resolve_url_path(
                "../fonts/ahem.css",
                Some(Path::new("http://127.0.0.1:8000/css/css-page")),
                Some(Path::new("http://127.0.0.1:8000")),
            )
            .unwrap(),
            PathBuf::from("http://127.0.0.1:8000/css/fonts/ahem.css")
        );
        assert_eq!(
            resolve_url_path(
                "/fonts/ahem.css",
                Some(Path::new("http://127.0.0.1:8000/css/css-page")),
                Some(Path::new("http://127.0.0.1:8000")),
            )
            .unwrap(),
            PathBuf::from("http://127.0.0.1:8000/fonts/ahem.css")
        );
    }
}
