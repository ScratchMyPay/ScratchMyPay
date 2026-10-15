//! RGB2link PowerEdge core: the AI proposes, this crate decides.
//!
//! Pipeline (all pure, no I/O except template loading):
//! 1. [`WorkflowIntent::parse`]: strict, size-bounded parse of untrusted model output.
//! 2. [`TemplateRegistry::resolve`]: exact id + version + SHA-256 digest; no nearest match.
//! 3. [`compile`]: typed slot binding (amounts converted with wallet precision, never the
//!    model's), deterministic node IDs, whole-string placeholder substitution only.
//! 4. [`validate`]: independent fail-closed checks: allowlisted node types, denylist,
//!    no secrets, wires intact, and no path to a spending node that skips approval.

pub mod canonical;
pub mod compile;
pub mod error;
pub mod intent;
pub mod registry;
pub mod validate;

pub use compile::{compile, AssetInfo, BindContext, CompiledFlow};
pub use error::{Error, Result};
pub use intent::{Environment, ParamValue, WorkflowIntent};
pub use registry::{Manifest, SlotKind, SlotSpec, Template, TemplateRegistry};
pub use validate::validate;
