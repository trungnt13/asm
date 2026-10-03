use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::CLIENT_ID;
use codex_login::CODEX_ACCESS_TOKEN_ENV_VAR;
use codex_login::REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR;
use codex_login::login_with_bedrock_access_keys;
use codex_models_manager::bundled_models_response;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::shell_environment::OPENAI_FEDERATION_RULE_ID_ENV_VAR;
use codex_protocol::shell_environment::OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

fn write_file_auth_config(codex_home: &Path) -> Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )?;
    Ok(())
}

fn read_auth_json(codex_home: &Path) -> Result<Value> {
    let auth_json = std::fs::read_to_string(codex_home.join("auth.json"))?;
    Ok(serde_json::from_str(&auth_json)?)
}

#[test]
fn login_with_api_key_reads_stdin_and_writes_auth_json() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "-c",
        "forced_login_method=\"api\"",
        "login",
        "--with-api-key",
    ])
    .write_stdin("sk-test\n")
    .assert()
    .success()
    .stderr(contains("Successfully logged in"));

    let auth = read_auth_json(codex_home.path())?;
    assert_eq!(auth["OPENAI_API_KEY"], "sk-test");
    assert!(auth.get("tokens").is_none());
    assert!(auth.get("agent_identity").is_none());

    Ok(())
}

#[test]
fn login_status_reports_auth_storage_errors() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;
    std::fs::write(codex_home.path().join("auth.json"), "{invalid json")?;

    codex_command(codex_home.path())?
        .args(["login", "status"])
        .assert()
        .failure()
        .stderr(contains("Error checking login status:"));

    Ok(())
}

#[test]
fn login_status_validates_configured_workload_identity() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;
    let missing_assertion = codex_home.path().join("missing-identity-token");

    codex_command(codex_home.path())?
        .env_remove(CODEX_ACCESS_TOKEN_ENV_VAR)
        .env(OPENAI_FEDERATION_RULE_ID_ENV_VAR, "rule-test")
        .env(OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR, &missing_assertion)
        .args(["login", "status"])
        .assert()
        .failure()
        .stderr(contains("could not read workload identity assertion file"));

    Ok(())
}

#[test]
fn logout_clears_only_the_selected_bedrock_provider() -> Result<()> {
    for (model_provider_id, managed_bedrock_auth, model) in [
        ("amazon-bedrock", true, "openai.gpt-5.6-sol"),
        ("amazon-bedrock-runtime", true, "global.openai.gpt-5.6-sol"),
        ("openai", true, "gpt-5.6-sol"),
        ("amazon-bedrock", false, "gpt-5.6-sol"),
        ("amazon-bedrock-runtime", false, "us.openai.gpt-5.6-sol"),
        ("openai", false, "gpt-5.6-sol"),
    ] {
        let codex_home = TempDir::new()?;
        let config_path = codex_home.path().join("config.toml");
        std::fs::write(
            &config_path,
            format!(
                "cli_auth_credentials_store = \"file\"\n\
                 model_provider = \"{model_provider_id}\"\n\
                 model = \"{model}\"\n\
                 model_reasoning_effort = \"high\"\n\
                 [model_providers.amazon-bedrock]\n\
                 base_url = \"https://mantle.example.com/v1\"\n\
                 [model_providers.amazon-bedrock.aws]\n\
                 profile = \"mantle-profile\"\n\
                 region = \"us-west-2\"\n\
                 auth_refresh = {{ command = \"aws\", args = [\"sso\", \"login\"] }}\n\
                 [model_providers.amazon-bedrock-runtime]\n\
                 base_url = \"https://runtime.example.com/v1\"\n\
                 [model_providers.amazon-bedrock-runtime.aws]\n\
                 profile = \"runtime-profile\"\n\
                 region = \"us-east-1\"\n\
                 auth_refresh = {{ command = \"aws\", args = [\"login\"] }}\n"
            ),
        )?;
        if managed_bedrock_auth {
            login_with_bedrock_access_keys(
                codex_home.path(),
                "managed-access-key-id",
                "managed-secret-access-key",
                Some("managed-session-token"),
                AuthCredentialsStoreMode::File,
                AuthKeyringBackendKind::default(),
            )?;
        }
        let mut expected_config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path)?)?;
        if model_provider_id != "openai" {
            let expected_root = expected_config
                .as_table_mut()
                .expect("config should be a table");
            expected_root.remove("model_provider");
            expected_root.remove("model");
            expected_root["model_providers"][model_provider_id]
                .as_table_mut()
                .expect("selected Bedrock provider should be a table")
                .remove("aws");
        }
        let expected_message = if managed_bedrock_auth || model_provider_id != "openai" {
            "Successfully logged out"
        } else {
            "Not logged in"
        };

        codex_command(codex_home.path())?
            .env_remove(CODEX_ACCESS_TOKEN_ENV_VAR)
            .env("AWS_ACCESS_KEY_ID", "environment-access-key-id")
            .env("AWS_SECRET_ACCESS_KEY", "environment-secret-access-key")
            .args(["logout"])
            .assert()
            .success()
            .stderr(contains(expected_message));

        assert!(!codex_home.path().join("auth.json").exists());
        let actual_config: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
        assert_eq!(actual_config, expected_config);
    }

    Ok(())
}

#[tokio::test]
async fn logout_survives_enterprise_cleanup_failure_with_xaa_disabled() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(path("/backend-api/wham/config/bundle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "config_toml": {"enterprise_managed": []},
            "requirements_toml": {"enterprise_managed": []},
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "cli_auth_credentials_store = \"file\"\nchatgpt_base_url = \"{}/backend-api\"\n[features]\nuse_xaa = false\n[mcp_enterprise_managed_auth.idp]\nissuer = \"https://idp.example\"\nclient_id = \"enterprise-client\"\n[mcp_servers.enterprise]\nurl = \"https://resource.example/mcp\"\nauth = \"ema_auth\"\nbearer_token_env_var = \"UNUSED_TOKEN\"\n",
            server.uri()
        ),
    )?;
    write_chatgpt_auth(
        home.path(),
        ChatGptAuthFixture::new("account-access")
            .account_id("workspace")
            .chatgpt_user_id("user"),
        AuthCredentialsStoreMode::File,
    )?;
    // Fail before touching the real keyring; the subprocess owns this isolated home.
    std::fs::write(home.path().join("mcp-oauth-locks"), "not a directory")?;
    for enabled in [false, true] {
        codex_command(home.path())?
            .current_dir(home.path())
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("CODEX_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove(CODEX_ACCESS_TOKEN_ENV_VAR)
            .args([
                "-c",
                &format!("features.use_xaa={enabled}"),
                "mcp",
                "logout",
                "enterprise",
            ])
            .assert()
            .failure()
            .stderr(contains("failed to delete enterprise authorization"));
        assert!(home.path().join("auth.json").exists());
    }
    codex_command(home.path())?
        .current_dir(home.path())
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .env(
            REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR,
            format!("{}/oauth/revoke", server.uri()),
        )
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove(CODEX_ACCESS_TOKEN_ENV_VAR)
        .args(["logout"])
        .assert()
        .success()
        .stderr(contains("continuing account logout"))
        .stderr(contains("Successfully logged out"));
    assert!(!home.path().join("auth.json").exists());
    Ok(())
}

#[test]
fn login_with_access_token_rejects_invalid_jwt() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["login", "--with-access-token"])
        .write_stdin("not-a-jwt\n")
        .assert()
        .failure()
        .stderr(contains("Error logging in with access token"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn debug_prompt_input_follows_authenticated_attribution_setting() -> Result<()> {
    let server = MockServer::start().await;
    let request_count = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/settings/user"))
        .and(header("chatgpt-account-id", "workspace-123"))
        .respond_with(move |_request: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(json!({
                "commit_attribution_enabled": request_count.fetch_add(1, Ordering::SeqCst) % 2 == 0,
            }))
        })
        .expect(4)
        .mount(&server)
        .await;
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!(
            "cli_auth_credentials_store = \"file\"\nchatgpt_base_url = \"{}/backend-api\"\n",
            server.uri()
        ),
    )?;
    write_chatgpt_auth(
        codex_home.path(),
        ChatGptAuthFixture::new("chatgpt-token")
            .account_id("workspace-123")
            .plan_type("enterprise"),
        AuthCredentialsStoreMode::File,
    )?;
    let output_schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false,
    });
    let output_schema_path = codex_home.path().join("output-schema.json");
    std::fs::write(&output_schema_path, serde_json::to_vec(&output_schema)?)?;
    let mut model_catalog = bundled_models_response()?;
    let model = model_catalog
        .models
        .iter_mut()
        .find(|model| model.slug == "gpt-5.5")
        .expect("bundled gpt-5.5 model");
    model.use_responses_lite = true;
    model.tool_mode = Some(ToolMode::Direct);
    let model_catalog_path = codex_home.path().join("model-catalog.json");
    std::fs::write(&model_catalog_path, serde_json::to_vec(&model_catalog)?)?;
    for (command, attribution_enabled, incremental_tools) in [
        ("prompt-input", true, false),
        ("prompt-input", false, false),
        ("prompt-request", true, false),
        ("prompt-request", false, true),
    ] {
        let mut cmd = codex_command(codex_home.path())?;
        cmd.env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("CODEX_ACCESS_TOKEN")
            .env_remove("OPENAI_API_KEY");
        if command == "prompt-request" {
            cmd.args([
                "-m",
                "gpt-5.5",
                "-c",
                "web_search=\"live\"",
                "-c",
                "features.goals=true",
                "-c",
            ])
            .arg(format!(
                "model_catalog_json={}",
                serde_json::to_string(&model_catalog_path)?
            ))
            .arg("-c")
            .arg(format!("features.incremental_tools={incremental_tools}"));
        }
        cmd.args(["debug", command]);
        if command == "prompt-request" {
            cmd.arg("--allow-session-state")
                .arg("--output-schema")
                .arg(&output_schema_path);
        }
        let output = cmd.output()?;
        assert!(output.status.success());
        let prompt = String::from_utf8(output.stdout)?;
        assert_eq!(
            prompt.contains("Co-authored-by: Codex <noreply@openai.com>"),
            attribution_enabled
        );
        assert!(!prompt.contains("attribution is disabled for the current workspace"));
        let prompt: Value = serde_json::from_str(&prompt)?;
        if command == "prompt-input" {
            assert!(prompt.is_array());
        } else {
            assert_eq!(prompt["schema_version"], 1);
            assert_eq!(prompt["scope"], "standalone_debug_turn");
            assert!(prompt["base_instructions"]["text"].is_string());
            assert!(prompt["request"]["input"].is_array());
            assert!(prompt["request"].get("tools").is_none());
            assert!(prompt["request"].get("instructions").is_none());
            let input = prompt["request"]["input"]
                .as_array()
                .expect("request input");
            let (tools_index, instructions_index) = if incremental_tools { (1, 0) } else { (0, 1) };
            assert_eq!(
                input
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item["type"] == "additional_tools")
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>(),
                vec![tools_index]
            );
            assert_eq!(input[tools_index]["role"], "developer");
            assert_eq!(input[instructions_index]["role"], "developer");
            assert_eq!(
                input[instructions_index]["content"][0]["text"],
                prompt["base_instructions"]["text"]
            );
            let tools = input[tools_index]["tools"]
                .as_array()
                .expect("request tool definitions");
            let web_run = tools
                .iter()
                .find(|tool| tool["name"] == "web")
                .and_then(|namespace| namespace["tools"].as_array())
                .and_then(|tools| tools.iter().find(|tool| tool["name"] == "run"))
                .expect("production web.run definition");
            assert_eq!(web_run["type"], "function");
            assert!(
                web_run["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
            assert!(web_run["parameters"].is_object());
            for (name, required) in [
                ("get_goal", json!([])),
                ("create_goal", json!(["objective"])),
                ("update_goal", json!(["status"])),
            ] {
                let goal_tool = tools
                    .iter()
                    .flat_map(|tool| {
                        tool["tools"]
                            .as_array()
                            .map_or_else(|| std::slice::from_ref(tool), Vec::as_slice)
                    })
                    .find(|tool| tool["name"] == name)
                    .expect("production goal tool definition");
                assert_eq!(goal_tool["type"], "function");
                assert!(
                    goal_tool["description"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
                );
                assert_eq!(goal_tool["parameters"]["type"], "object");
                assert_eq!(goal_tool["parameters"]["required"], required);
                assert_eq!(goal_tool["parameters"]["additionalProperties"], false);
                let properties = &goal_tool["parameters"]["properties"];
                match name {
                    "get_goal" => assert_eq!(properties, &json!({})),
                    "create_goal" => {
                        assert_eq!(properties["objective"]["type"], "string");
                        assert_eq!(properties["token_budget"]["type"], "integer");
                    }
                    "update_goal" => assert_eq!(
                        properties["status"]["enum"],
                        json!(["complete", "blocked", "paused"])
                    ),
                    _ => unreachable!("goal names are listed above"),
                }
            }
            assert_eq!(
                prompt["request"]["text"]["format"],
                json!({
                    "type": "json_schema",
                    "name": "codex_output_schema",
                    "strict": true,
                    "schema": output_schema,
                })
            );
        }
    }
    assert!(
        server
            .received_requests()
            .await
            .expect("mock request recording is enabled")
            .iter()
            .all(|request| !(request.method.as_str() == "POST"
                && request.url.path().ends_with("/responses")))
    );
    server.verify().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_login_revokes_existing_auth_before_requesting_new_tokens() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_auth_id": "device-auth-123",
            "user_code": "CODE-12345",
            "interval": "0",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorization_code": "authorization-code-123",
            "code_challenge": "code-challenge-123",
            "code_verifier": "code-verifier-123",
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id_token": "eyJhbGciOiJub25lIn0.e30.c2ln",
            "access_token": "new-access",
            "refresh_token": "new-refresh",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let codex_home = TempDir::new()?;
    write_file_auth_config(codex_home.path())?;
    std::fs::write(
        codex_home.path().join("auth.json"),
        serde_json::to_vec(&json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "eyJhbGciOiJub25lIn0.e30.c2ln",
                "access_token": "old-access",
                "refresh_token": "old-refresh",
                "account_id": "old-account",
            },
        }))?,
    )?;

    let issuer = server.uri();
    let mut cmd = codex_command(codex_home.path())?;
    cmd.env(
        REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR,
        format!("{issuer}/oauth/revoke"),
    )
    .env("NO_PROXY", "127.0.0.1,localhost")
    .env("no_proxy", "127.0.0.1,localhost")
    .env_remove("CODEX_ACCESS_TOKEN")
    .env_remove("OPENAI_API_KEY")
    .args(["login", "--device-auth", "--experimental_issuer", &issuer])
    .assert()
    .success()
    .stderr(contains("Successfully logged in"));

    let requests = server
        .received_requests()
        .await
        .context("failed to read mock OAuth requests")?;
    let paths: Vec<&str> = requests.iter().map(|request| request.url.path()).collect();
    assert_eq!(
        paths,
        vec![
            "/oauth/revoke",
            "/api/accounts/deviceauth/usercode",
            "/api/accounts/deviceauth/token",
            "/oauth/token",
        ]
    );
    assert_eq!(
        requests[0]
            .body_json::<Value>()
            .context("revoke request should be JSON")?,
        json!({
            "token": "old-refresh",
            "token_type_hint": "refresh_token",
            "client_id": CLIENT_ID,
        })
    );

    let auth = read_auth_json(codex_home.path())?;
    assert_eq!(auth["tokens"]["refresh_token"], "new-refresh");
    Ok(())
}
