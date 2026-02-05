//! Embedding module for semantic search.
//!
//! When compiled with the `embeddings` feature (default), provides a singleton
//! `Embedder` backed by fastembed (BGE-large-en-v1.5, 1024d). Downloads the
//! ONNX model on first use (~335MB, cached at `~/.cache/fastembed/`).
//!
//! Without the `embeddings` feature, all methods are no-ops that return `None`,
//! so the knowledge graph still works with text-based search fallback.
//!
//! Can also be disabled at runtime via `[knowledge] embeddings_enabled = false`
//! in config.toml.

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "embeddings")]
use std::sync::OnceLock;

#[cfg(feature = "embeddings")]
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Singleton model — expensive to load, reused across all calls.
#[cfg(feature = "embeddings")]
static EMBEDDING_MODEL: OnceLock<Option<TextEmbedding>> = OnceLock::new();

/// Only warn about unavailability once per process.
static EMBEDDINGS_WARNED: AtomicBool = AtomicBool::new(false);

/// Set to true to globally disable embedding attempts (via config).
static EMBEDDINGS_DISABLED: AtomicBool = AtomicBool::new(false);

pub struct Embedder;

impl Embedder {
    /// Globally disable embeddings for this process.
    /// Called at startup when `[knowledge] embeddings_enabled = false`.
    pub fn disable() {
        EMBEDDINGS_DISABLED.store(true, Ordering::Relaxed);
    }

    /// Begin loading the embedding model in a background thread.
    /// Call once at startup. The model becomes available when loading completes;
    /// until then, `embed()` / `available()` gracefully return `None` / `false`
    /// so the first chat message is never blocked by model init.
    #[cfg(feature = "embeddings")]
    pub fn init_background() {
        if EMBEDDINGS_DISABLED.load(Ordering::Relaxed) {
            return;
        }
        std::thread::spawn(|| {
            let _ = EMBEDDING_MODEL.get_or_init(|| {
                let mut opts = InitOptions::default();
                opts.model_name = EmbeddingModel::BGELargeENV15;
                opts.show_download_progress = true;
                match TextEmbedding::try_new(opts) {
                    Ok(model) => Some(model),
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to load embedding model — using text search fallback");
                        None
                    }
                }
            });
        });
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn init_background() {
        // No-op: fastembed not compiled in
    }

    /// Non-blocking check for the embedding model.
    /// Returns `None` if disabled, not yet loaded (background init in progress),
    /// or if loading failed. Always returns `None` without the `embeddings` feature.
    #[cfg(feature = "embeddings")]
    fn try_model() -> Option<&'static TextEmbedding> {
        if EMBEDDINGS_DISABLED.load(Ordering::Relaxed) {
            return None;
        }
        // Non-blocking: returns None while background init is still running
        EMBEDDING_MODEL.get()?.as_ref()
    }

    #[cfg(not(feature = "embeddings"))]
    fn try_model() -> Option<&'static ()> {
        None
    }

    /// Embed a single piece of text. Returns `None` when the model is
    /// unavailable, disabled, or not compiled in.
    pub fn embed(text: &str) -> Option<Vec<f32>> {
        #[cfg(feature = "embeddings")]
        {
            let model = Self::try_model()?;
            model.embed(vec![text], None).ok().and_then(|mut v| v.pop())
        }
        #[cfg(not(feature = "embeddings"))]
        {
            let _ = text;
            None
        }
    }

    /// Embed multiple texts in one batch (more efficient than repeated single
    /// calls). Returns `None` when the model is unavailable, disabled, or not
    /// compiled in.
    pub fn embed_batch(texts: Vec<&str>) -> Option<Vec<Vec<f32>>> {
        #[cfg(feature = "embeddings")]
        {
            let model = Self::try_model()?;
            model.embed(texts, None).ok()
        }
        #[cfg(not(feature = "embeddings"))]
        {
            let _ = texts;
            None
        }
    }

    /// Whether the embedding model is loaded and ready.
    pub fn available() -> bool {
        Self::try_model().is_some()
    }

    /// Print a one-time warning if embeddings aren't available.
    pub fn warn_if_unavailable() {
        if !Self::available() && !EMBEDDINGS_WARNED.swap(true, Ordering::Relaxed) {
            if EMBEDDINGS_DISABLED.load(Ordering::Relaxed) {
                eprintln!("ℹ  Embeddings disabled via config — using text search fallback");
            } else if cfg!(feature = "embeddings") {
                eprintln!("⚠️  Semantic search disabled (embedding model unavailable)");
            } else {
                eprintln!("ℹ  Built without embeddings feature — using text search fallback");
            }
        }
    }
}
