use sisyphus_tools::{OpenAiLlmClient, metis::LlmClient};
use std::env;

#[tokio::test]
async fn test_gemma4_availability() {
    let api_key = env::var("GOOGLE_GENERATIVE_AI_API_KEY")
        .expect("GOOGLE_GENERATIVE_AI_API_KEY environment variable must be set");
    
    let endpoint = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string();
    let model = "gemma-4-31b-it".to_string();
    
    let client = OpenAiLlmClient::new(api_key, endpoint, model);
    
    println!("Testing availability for model: gemma-4-31b-it");
    
    let system = "You are a helpful assistant.";
    let user = "Hello! Are you available? Please respond with 'YES' if you can hear me.";
    
    match client.chat(system, user, false).await {
        Ok(response) => {
            println!("Response received: {}", response);
            assert!(!response.is_empty(), "Response was empty");
        },
        Err(e) => {
            panic!("LLM call failed: {}", e);
        }
    }
}
