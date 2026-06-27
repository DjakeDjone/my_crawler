use anyhow::{Context, Result};
use qdrant_client::{
    qdrant::{
        Condition, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, DeletePointsBuilder,
        Distance, DocumentBuilder, FieldType, Filter, Modifier, NamedVectors, PointStruct,
        SparseIndexConfigBuilder, SparseVectorParamsBuilder, SparseVectorsConfigBuilder,
        UpsertPointsBuilder, VectorParamsBuilder, VectorsConfigBuilder,
    },
    Payload, Qdrant,
};
use reqwest::Client;
use serde::Serialize;
use sha2::{Digest, Sha256};
use shared_crawler_api::{WebPageChunk, QDRANT_COLLECTION_NAME};
use std::{collections::HashMap, env};
use uuid::Uuid;

const BM25_MODEL: &str = "qdrant/bm25";

pub struct PageIndexer {
    qdrant: Qdrant,
    http: Client,
    tei_url: String,
}

impl PageIndexer {
    pub fn from_env() -> Result<Self> {
        let qdrant_url =
            env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
        Ok(Self {
            qdrant: Qdrant::from_url(&qdrant_url).build()?,
            http: Client::new(),
            tei_url: env::var("TEI_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        })
    }

    pub async fn ensure_collection(&self) -> Result<()> {
        if self
            .qdrant
            .collection_exists(QDRANT_COLLECTION_NAME)
            .await?
        {
            return Ok(());
        }
        let mut dense = VectorsConfigBuilder::default();
        dense.add_named_vector_params(
            "dense",
            VectorParamsBuilder::new(384, Distance::Cosine).on_disk(true),
        );
        let sparse_params = SparseVectorParamsBuilder::default()
            .modifier(Modifier::Idf)
            .index(SparseIndexConfigBuilder::default().on_disk(true));
        let mut sparse = SparseVectorsConfigBuilder::default();
        sparse.add_named_vector_params("title_bm25", sparse_params.clone());
        sparse.add_named_vector_params("body_bm25", sparse_params);
        self.qdrant
            .create_collection(
                CreateCollectionBuilder::new(QDRANT_COLLECTION_NAME)
                    .vectors_config(dense)
                    .sparse_vectors_config(sparse)
                    .on_disk_payload(true),
            )
            .await?;
        for (field, kind) in [
            ("source_url", FieldType::Keyword),
            ("page_version", FieldType::Keyword),
            ("crawled_at", FieldType::Integer),
            ("chunk_index", FieldType::Integer),
        ] {
            self.qdrant
                .create_field_index(
                    CreateFieldIndexCollectionBuilder::new(QDRANT_COLLECTION_NAME, field, kind)
                        .wait(true),
                )
                .await?;
        }
        Ok(())
    }

    pub async fn index_page(&self, chunks: &[WebPageChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let source_url = &chunks[0].source_url;
        let version = page_version(chunks);
        let documents = chunks
            .iter()
            .map(|chunk| {
                format!(
                    "passage: {}\n{}\n{}",
                    chunk.page_title,
                    chunk.chunk_heading.as_deref().unwrap_or(""),
                    chunk.chunk_content
                )
            })
            .collect::<Vec<_>>();
        let dense = self.embed(&documents).await?;
        if dense.len() != chunks.len() || dense.iter().any(|vector| vector.len() != 384) {
            anyhow::bail!("TEI returned invalid embedding dimensions");
        }

        let points = chunks
            .iter()
            .zip(dense)
            .enumerate()
            .map(|(index, (chunk, dense))| {
                let title = format!(
                    "{}\n{}",
                    chunk.page_title,
                    chunk.chunk_heading.as_deref().unwrap_or("")
                );
                let body = format!("{}\n{}", chunk.description, chunk.chunk_content);
                let mut payload = chunk.to_payload_json();
                let object = payload.as_object_mut().unwrap();
                object.insert("page_version".to_string(), version.clone().into());
                object.insert("chunk_index".to_string(), (index as i64).into());
                PointStruct::new(
                    point_id(source_url, &version, index),
                    NamedVectors::default()
                        .add_vector("dense", dense)
                        .add_vector("title_bm25", bm25_document(title))
                        .add_vector("body_bm25", bm25_document(body)),
                    Payload::try_from(payload).unwrap(),
                )
            })
            .collect::<Vec<_>>();

        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(QDRANT_COLLECTION_NAME, points).wait(true))
            .await?;
        self.qdrant
            .delete_points(
                DeletePointsBuilder::new(QDRANT_COLLECTION_NAME)
                    .points(stale_version_filter(source_url, &version))
                    .wait(true),
            )
            .await?;
        Ok(())
    }

    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let response = self
            .http
            .post(format!("{}/embed", self.tei_url.trim_end_matches('/')))
            .json(&EmbedRequest { inputs })
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<Vec<f32>>>()
            .await
            .context("invalid TEI response")?;
        Ok(response)
    }
}

fn bm25_document(text: String) -> qdrant_client::qdrant::Document {
    DocumentBuilder::new(text, BM25_MODEL)
        .options(HashMap::from([("language".to_string(), "none".into())]))
        .build()
}

fn page_version(chunks: &[WebPageChunk]) -> String {
    let mut hasher = Sha256::new();
    for chunk in chunks {
        hasher.update(chunk.page_title.as_bytes());
        hasher.update(chunk.chunk_heading.as_deref().unwrap_or("").as_bytes());
        hasher.update(chunk.description.as_bytes());
        hasher.update(chunk.chunk_content.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn stale_version_filter(source_url: &str, version: &str) -> Filter {
    Filter {
        must: vec![Condition::matches("source_url", source_url.to_string())],
        must_not: vec![Condition::matches("page_version", version.to_string())],
        ..Default::default()
    }
}

fn point_id(url: &str, version: &str, index: usize) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("{url}\0{version}\0{index}").as_bytes(),
    )
    .to_string()
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_versions_and_ids_are_stable() {
        let chunk = WebPageChunk::new(
            "content".into(),
            None,
            "https://example.com".into(),
            "title".into(),
            String::new(),
            vec![],
            vec![],
            0.0,
            0.0,
            0,
        );
        let version = page_version(std::slice::from_ref(&chunk));
        assert_eq!(version, page_version(&[chunk]));
        assert_eq!(point_id("u", &version, 0), point_id("u", &version, 0));
        let filter = stale_version_filter("u", &version);
        assert_eq!(filter.must.len(), 1);
        assert_eq!(filter.must_not.len(), 1);
    }
}
