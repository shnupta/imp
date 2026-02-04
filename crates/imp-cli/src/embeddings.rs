//! Embedding module for semantic search.
//!
//! Provides a singleton `Embedder` backed by fastembed (BGE-large-en-v1.5, 1024d).
//! Downloads the ONNX model on first use (~335MB, cached at `~/.cache/fastembed/`).
//! Gracefully degrades if the model can't be loaded — returns `None` and logs a
//! warning once so the rest of the knowledge graph still works.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Singleton model — expensive to load, reused across all calls.
static EMBEDDING_MODEL: OnceLock<Option<TextEmbedding>> = OnceLock::new();

/// Only warn about unavailability once per process.
static EMBEDDINGS_WARNED: AtomicBool = AtomicBool::new(false);

pub struct Embedder;

impl Embedder {
    /// Get or lazily initialise the embedding model.
    /// Returns `None` if the model couldn't be loaded (download failure, ONNX
    /// issues, disk full, etc.).
    fn try_model() -> Option<&'static TextEmbedding> {
        EMBEDDING_MODEL
            .get_or_init(|| {
                let mut opts = InitOptions::default();
                opts.model_name = EmbeddingModel::BGELargeENV15;
                opts.show_download_progress = true;
                match TextEmbedding::try_new(opts) {
                    Ok(model) => Some(model),
                    Err(e) => {
                        eprintln!("⚠️  Failed to load embedding model: {e}");
                        eprintln!("   Knowledge graph will work without semantic search.");
                        eprintln!("   Entity lookup and graph traversal still available.");
                        None
                    }
                }
            })
            .as_ref()
    }

    /// Embed a single piece of text. Returns `None` when the model is
    /// unavailable.
    pub fn embed(text: &str) -> Option<Vec<f32>> {
        let model = Self::try_model()?;
        model
            .embed(vec![text], None)
            .ok()
            .and_then(|mut v| v.pop())
    }

    /// Embed multiple texts in one batch (more efficient than repeated single
    /// calls). Returns `None` when the model is unavailable.
    pub fn embed_batch(texts: Vec<&str>) -> Option<Vec<Vec<f32>>> {
        let model = Self::try_model()?;
        model.embed(texts, None).ok()
    }

    /// Whether the embedding model is loaded and ready.
    pub fn available() -> bool {
        Self::try_model().is_some()
    }

    /// Print a one-time warning if embeddings aren't available.
    pub fn warn_if_unavailable() {
        if !Self::available() && !EMBEDDINGS_WARNED.swap(true, Ordering::Relaxed) {
            eprintln!("⚠️  Semantic search disabled (embedding model unavailable)");
        }
    }
}
