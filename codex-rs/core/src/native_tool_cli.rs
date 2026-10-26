use anyhow::Context;
use anyhow::bail;
use serde_json::Value;
use serde_json::json;
use std::io::Read;
use std::io::Write;

/// Runs the child-only communication helper without loading CLI configuration.
pub fn run_native_tool_cli() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(call_native_tool());
    let response = match &result {
        Ok(response) => response.clone(),
        Err(error) => json!({"isError":true,"error":error.to_string()}),
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response)?;
    writeln!(stdout)?;
    result?;
    if response["isError"] == true {
        bail!("ASM tool returned an error");
    }
    Ok(())
}

#[tracing::instrument(skip_all, err)]
async fn call_native_tool() -> anyhow::Result<Value> {
    let arguments = std::env::args().skip(/*n*/ 2).collect::<Vec<_>>();
    let descriptor = std::env::var_os("ASM_NATIVE_DESCRIPTOR")
        .context("ASM native tools are available only inside managed native children")?;
    let descriptor: Value = serde_json::from_slice(&std::fs::read(descriptor)?)?;
    let url = descriptor["url"]
        .as_str()
        .context("Missing ASM bridge URL")?;
    let token = descriptor["token"]
        .as_str()
        .context("Missing ASM bridge credential")?;
    let (route, body) = match arguments.as_slice() {
        [command] if command == "list" => ("/tools", json!({})),
        [command, name] if command == "call" => {
            let mut input = String::new();
            std::io::stdin()
                .take(/*limit*/ 1024 * 1024 + 1)
                .read_to_string(&mut input)?;
            if input.len() > 1024 * 1024 {
                bail!("ASM tool arguments exceed 1 MiB");
            }
            let arguments: Value = serde_json::from_str(&input)?;
            ("/call", json!({"name":name,"arguments":arguments}))
        }
        _ => bail!("Usage: --asm-native-tools list | call <name> (JSON arguments on stdin)"),
    };
    // Child credentials must stay on the local connection, not an inherited proxy.
    let client = codex_client::HttpClientBuilder::new()
        .without_redirects()
        .without_request_logging()
        .build_direct()?;
    let response: Value = client
        .post(format!("{url}{route}"))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_millis(
            descriptor["timeoutMs"]
                .as_u64()
                .context("Missing ASM bridge timeout")?,
        ))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response)
}
