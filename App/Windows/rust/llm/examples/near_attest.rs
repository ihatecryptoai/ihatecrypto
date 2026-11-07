//! LLMCheck's `nearai --attest` (`LLM/Check/main.swift`): NEAR AI Cloud's enclaves checked the way the chat checks them
//! before a message. It needs no key.
//!   cargo run -p llm --example near_attest [model]       (or IHATECRYPTO_LLM_MODEL=…; the provider's default otherwise)

#[tokio::main]
async fn main() {
    let model = std::env::args().nth(1).or_else(|| std::env::var("IHATECRYPTO_LLM_MODEL").ok())
        .unwrap_or_else(|| llm::near_ai::info().default_model);
    match llm::near_ai::ensure_verified(&model).await {
        Ok(v) => println!("Verified {}: the gateway's and the model's Intel TDX quotes are signed down to Intel's root and bound to a \
                           fresh nonce and to the keys below, their configuration and event log match, and NVIDIA passed the GPUs.\n  \
                           gateway key {}\n  model key   {}", v.model, v.gateway_key, v.model_keys.join(", ")),
        Err(e) => {
            eprintln!("Not verified: {e}");
            std::process::exit(1)
        }
    }
}
