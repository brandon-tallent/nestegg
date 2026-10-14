//! Shared mirror data and calculations for the terminal app and MCP service.
pub mod data;
pub mod equity;
pub mod forecast;

#[cfg(feature = "mcp")]
pub mod mcp;
