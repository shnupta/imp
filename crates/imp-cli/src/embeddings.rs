//! Embedding module for semantic search.
//!
//! Provides a singleton `Embedder` backed by fastembed (BGE-large-en-v1.5, 1024d).
//! Downloads the ONNX model on first use (~335MB, cached at `~/.cache/fastembed/`).
//! Gracefully degrades if the model can't be loaded — returns `None` and logs a
//! warning once so the rest of the knowledge graph still works.
//!
//! Can be disabled entirely via `[knowledge] embeddings_enabled = false` in
//! config.toml to avoid the model download (e.g. behind a firewall).

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Singleton model — expensive to load, reused across all calls.
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
                        eprintln!("⚠️  Failed to load embedding model: {e}");
                        eprintln!("   Knowledge graph will work without semantic search.");
                        eprintln!("   Entity lookup and graph traversal still available.");
                        eprintln!("   To disable this warning, set embeddings_enabled = false");
                        eprintln!("   in [knowledge] section of ~/.imp/config.toml");
                        None
                    }
                }
            });
        });
    }

    /// Non-blocking check for the embedding model.
    /// Returns `None` if disabled, not yet loaded (background init in progress),
    /// or if loading failed.
    fn try_model() -> Option<&'static TextEmbedding> {
        if EMBEDDINGS_DISABLED.load(Ordering::Relaxed) {
            return None;
        }
        // Non-blocking: returns None while background init is still running
        EMBEDDING_MODEL.get()?.as_ref()
    }

    /// Embed a single piece of text. Returns `None` when the model is
    /// unavailable or disabled.
    pub fn embed(text: &str) -> Option<Vec<f32>> {
        let model = Self::try_model()?;
        model
            .embed(vec![text], None)
            .ok()
            .and_then(|mut v| v.pop())
    }

    /// Embed multiple texts in one batch (more efficient than repeated single
    /// calls). Returns `None` when the model is unavailable or disabled.
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
            if EMBEDDINGS_DISABLED.load(Ordering::Relaxed) {
                eprintln!("ℹ  Embeddings disabled via config — using text search fallback");
            } else {
                eprintln!("⚠️  Semantic search disabled (embedding model unavailable)");
            }
        }
    }
}
