//! Provider compatibility adapters.
//!
//! When a provider claims to be "OpenAI-compatible" but enforces a stricter
//! subset of the spec, the native rig provider may serialize requests that
//! the endpoint rejects. Each submodule here owns the exact wire contract for
//! one such provider.
//!
//! Rule: Rig is the app's LLM abstraction. Local adapters own the wire format.

pub mod kimi;
