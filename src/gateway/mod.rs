//! Gateway — HTTP/WebSocket server for remote agent management
//! Allows external tools, editors, and clients to interact with agents

pub mod server;

pub use server::{
    start_gateway, start_gateway_with_runtime, ChatRequest, ChatResponse, ContainerStatus, Gateway,
};
