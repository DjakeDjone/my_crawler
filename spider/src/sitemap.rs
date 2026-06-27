use anyhow::{bail, Result};
use flate2::read::GzDecoder;
use sitemap::reader::{SiteMapEntity, SiteMapReader};
use std::{
    collections::{HashSet, VecDeque},
    io::{Cursor, Read},
    sync::Arc,
};
use url::Url;

use crate::web_visitor::{normalize_url, same_origin, FetchResult, WebVisitorImpl};

const MAX_DEPTH: usize = 3;
const MAX_FILES: usize = 20;
const MAX_DECOMPRESSED_BYTES: usize = 10 * 1024 * 1024;

pub async fn discover(
    visitor: Arc<WebVisitorImpl>,
    seed: &Url,
    declared: Vec<Url>,
    max_pages: usize,
) -> Vec<Url> {
    let initial = if declared.is_empty() {
        seed.join("/sitemap.xml").into_iter().collect()
    } else {
        declared
    };
    let mut queue = initial
        .into_iter()
        .filter(|url| same_origin(seed, url))
        .map(|url| (url, 0usize))
        .collect::<VecDeque<_>>();
    let mut files = HashSet::new();
    let mut pages = Vec::new();
    let limit = max_pages.saturating_mul(4).min(10_000);

    while let Some((url, depth)) = queue.pop_front() {
        if files.len() >= MAX_FILES || pages.len() >= limit || depth > MAX_DEPTH {
            break;
        }
        if !files.insert(url.to_string()) {
            continue;
        }
        let Ok(result) = visitor
            .fetch_resource(url.as_str(), MAX_DECOMPRESSED_BYTES, 10)
            .await
        else {
            continue;
        };
        if !result.status.is_success() {
            continue;
        }
        let Ok((mut found_pages, mut nested)) = parse_response(&url, result) else {
            continue;
        };
        found_pages.retain(|(page, _)| same_origin(seed, page));
        found_pages.sort_by(|left, right| right.1.cmp(&left.1));
        pages.extend(found_pages.into_iter().map(|(url, _)| url));
        pages.truncate(limit);

        if depth < MAX_DEPTH {
            nested.retain(|(map, _)| same_origin(seed, map));
            nested.sort_by(|left, right| right.1.cmp(&left.1));
            queue.extend(nested.into_iter().map(|(url, _)| (url, depth + 1)));
        }
    }

    let mut seen = HashSet::new();
    pages
        .into_iter()
        .filter_map(|url| normalize_url(url.as_str()))
        .filter(|url| seen.insert(url.to_string()))
        .take(limit)
        .collect()
}

type DatedUrl = (Url, Option<i64>);

fn parse_response(url: &Url, result: FetchResult) -> Result<(Vec<DatedUrl>, Vec<DatedUrl>)> {
    let gzip = url.path().ends_with(".gz")
        || result
            .content_type
            .as_ref()
            .is_some_and(|value| value.essence_str() == "application/gzip");
    parse_document(&result.body, gzip)
}

fn parse_document(bytes: &[u8], gzip: bool) -> Result<(Vec<DatedUrl>, Vec<DatedUrl>)> {
    let mut xml = Vec::new();
    if gzip {
        GzDecoder::new(bytes)
            .take((MAX_DECOMPRESSED_BYTES + 1) as u64)
            .read_to_end(&mut xml)?;
    } else {
        xml.extend_from_slice(bytes);
    }
    if xml.len() > MAX_DECOMPRESSED_BYTES {
        bail!("sitemap exceeds decompressed size limit");
    }

    let mut pages = Vec::new();
    let mut nested = Vec::new();
    for entity in SiteMapReader::new(Cursor::new(xml)) {
        match entity {
            SiteMapEntity::Url(entry) => {
                if let Some(url) = entry.loc.get_url() {
                    pages.push((url, entry.lastmod.get_time().map(|value| value.timestamp())));
                }
            }
            SiteMapEntity::SiteMap(entry) => {
                if let Some(url) = entry.loc.get_url() {
                    nested.push((url, entry.lastmod.get_time().map(|value| value.timestamp())));
                }
            }
            SiteMapEntity::Err(error) => return Err(error.into()),
        }
    }
    Ok((pages, nested))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    #[test]
    fn parses_url_sets_indexes_and_gzip() {
        let urls = br#"<urlset><url><loc>https://example.com/old</loc><lastmod>2024-01-01</lastmod></url></urlset>"#;
        assert_eq!(parse_document(urls, false).unwrap().0.len(), 1);

        let index = br#"<sitemapindex><sitemap><loc>https://example.com/a.xml</loc></sitemap></sitemapindex>"#;
        assert_eq!(parse_document(index, false).unwrap().1.len(), 1);

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(urls).unwrap();
        assert_eq!(
            parse_document(&encoder.finish().unwrap(), true)
                .unwrap()
                .0
                .len(),
            1
        );
    }
}
