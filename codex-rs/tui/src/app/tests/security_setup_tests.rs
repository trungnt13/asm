//! Verifies ASM suppresses optional reminders without changing account behavior.
use super::disconnect::serve_reconnect_requests;
use super::*;
use crate::app_server_session::ThreadParamsMode;
use crate::security_setup::Identity;
use crate::status::StatusAccountDisplay;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::AccountUpdatedNotification;
use codex_app_server_protocol::AuthMode;
use codex_login::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::test]
async fn security_setup_disabled_preserves_account_email_and_backend_banners() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let backend = wiremock::MockServer::start().await;
    app.config.chatgpt_base_url = backend.uri();
    app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
    std::fs::write(
        app.config.codex_home.join("config.toml"),
        format!("chatgpt_base_url = {:?}\n", backend.uri()),
    )?;
    write_chatgpt_auth(
        &app.config.codex_home,
        ChatGptAuthFixture::new("test-token")
            .email("user@example.com")
            .account_id("account")
            .chatgpt_user_id("user"),
        AuthCredentialsStoreMode::File,
    )
    .expect("write synthetic auth");
    app_test_support::mount_workspace_routing(&backend).await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    wiremock::Mock::given(wiremock::matchers::path("/wham/security-setup"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .expect(0)
        .mount(&backend)
        .await;
    let request_id = app.chat_widget.security_setup_request_id;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let init = app.chatwidget_init_for_forked_or_resumed_thread(
        &mut tui,
        app.config.clone(),
        /*initial_user_message*/ None,
    );
    app.replace_chat_widget(ChatWidget::new_with_app_event(init));
    assert_eq!(app.chat_widget.security_setup_request_id, request_id);
    let (tx, mut reminder_events) = mpsc::unbounded_channel();
    crate::security_setup::prefetch(&app.config, &server, AppEventSender::new(tx), request_id);
    // No task retains the sender: suppression happens before auth or HTTP requests.
    assert!(
        tokio::time::timeout(Duration::from_secs(5), reminder_events.recv())
            .await?
            .is_none()
    );
    let notice: crate::security_setup::Notice = serde_json::from_value(json!({
        "title": "Set up security for Daybreak mode", "description": "Set up security.",
        "action": {"label": "Set up security", "url": "https://chatgpt.com/cyber"}
    }))?;
    let identity = Identity {
        account: "account".into(),
        user: "user".into(),
    };
    app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::SecuritySetupLoaded {
            request_id,
            identity: identity.clone(),
            notice: notice.clone(),
        },
    )
    .await?;
    assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 70).contains("Set up security"));

    // Reconnect can emit AccountUpdated for the same identity more than once.
    for _ in 0..2 {
        let stale_request_id = app.chat_widget.security_setup_request_id;
        app.handle_app_server_event(
            &server,
            AppServerEvent::ServerNotification(Box::new(ServerNotification::AccountUpdated(
                AccountUpdatedNotification {
                    auth_mode: Some(AuthMode::Chatgpt),
                    plan_type: None,
                },
            ))),
        )
        .await;
        let request_id = app.chat_widget.security_setup_request_id;
        assert_ne!(request_id, stale_request_id);
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::SecuritySetupLoaded {
                request_id: stale_request_id,
                identity: Identity {
                    account: "stale-account".into(),
                    ..identity.clone()
                },
                notice: notice.clone(),
            },
        )
        .await?;
        assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 70).contains("Set up security"));

        let init = app.chatwidget_init_for_forked_or_resumed_thread(
            &mut tui,
            app.config.clone(),
            /*initial_user_message*/ None,
        );
        app.replace_chat_widget(ChatWidget::new_with_app_event(init));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = events.recv().await.unwrap();
                assert!(!matches!(event, AppEvent::SecuritySetupLoaded { .. }));
                if matches!(event, AppEvent::AccountEmailLoaded { .. }) {
                    app.handle_event(&mut tui, &mut server, event).await?;
                    break Result::<()>::Ok(());
                }
            }
        })
        .await??;
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::AccountEmailLoaded {
                request_id: uuid::Uuid::new_v4(),
                email: Some("stale@example.com".into()),
            },
        )
        .await?;
        assert_eq!(
            app.chat_widget.status_account_display(),
            Some(&StatusAccountDisplay::ChatGpt {
                email: Some("user@example.com".into()),
                plan: None,
            })
        );
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::SecuritySetupLoaded {
                request_id,
                identity: identity.clone(),
                notice: notice.clone(),
            },
        )
        .await?;
        assert!(!render_bottom_popup(&app.chat_widget, /*width*/ 70).contains("Set up security"));
    }
    app.chat_widget.set_model("test-model-a");
    let response = codex_app_server_protocol::GetAccountRateLimitsResponse {
        ordinary_usage_allowed: None,
        account_id: Some("account".into()),
        rate_limit_upsell: Some(json!({
            "banner_type": "selected_model_limit", "model_slug": "test-model-a",
            "title": "Selected model usage exhausted", "description": "Switch models.",
            "presentation": "dismissible", "ctas": [],
        })),
        rate_limits: serde_json::from_value(json!({}))?,
        rate_limits_by_limit_id: None,
        rate_limit_reset_credits: None,
    };
    app.chat_widget.update_backend_banner(&response);
    assert!(
        render_bottom_popup(&app.chat_widget, /*width*/ 70)
            .contains("Selected model usage exhausted")
    );
    let request_id = app.chat_widget.security_setup_request_id;
    app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::SecuritySetupLoaded {
            request_id,
            identity,
            notice,
        },
    )
    .await?;
    assert!(
        render_bottom_popup(&app.chat_widget, /*width*/ 70)
            .contains("Selected model usage exhausted")
    );
    backend.verify().await;
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn security_setup_disabled_skips_auth_status_and_http_requests() -> Result<()> {
    for (auth_method, auth_token) in [
        (
            Some(AuthMode::ChatgptAuthTokens),
            Some("other-account-token"),
        ),
        (Some(AuthMode::ChatgptAuthTokens), Some("saved-token")),
        (Some(AuthMode::Chatgpt), Some("other-account-token")),
        (Some(AuthMode::Chatgpt), Some("saved-token")),
        (Some(AuthMode::Chatgpt), None),
        (None, None),
    ] {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let backend = wiremock::MockServer::start().await;
        app.config.chatgpt_base_url = backend.uri();
        app.config.cli_auth_credentials_store_mode = AuthCredentialsStoreMode::File;
        write_chatgpt_auth(
            &app.config.codex_home,
            ChatGptAuthFixture::new("saved-token")
                .account_id("saved-account")
                .chatgpt_user_id("saved-user"),
            AuthCredentialsStoreMode::File,
        )
        .expect("write synthetic auth");
        wiremock::Mock::given(wiremock::matchers::path("/wham/security-setup"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "notice": {
                    "title": "Keep using Daybreak mode", "description": "Set up security.",
                    "action": {"label": "Set up security", "url": "https://chatgpt.com/cyber"}
                }
            })))
            .expect(0)
            .mount(&backend)
            .await;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
        let daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            serve_reconnect_requests(tokio_tungstenite::accept_async(stream).await?, |request| {
                assert_eq!(request.method, "getAuthStatus");
                assert_eq!(
                    request.params,
                    Some(json!({"includeToken": true, "refreshToken": false}))
                );
                std::future::ready(Some(json!({"result": {
                    "authMethod": auth_method, "authToken": auth_token,
                    "requiresOpenaiAuth": true
                }})))
            })
            .await
        });
        let server = AppServerSession::new(
            crate::connect_remote_app_server(endpoint).await?,
            ThreadParamsMode::Embedded,
        );
        let (tx, mut events) = mpsc::unbounded_channel();
        crate::security_setup::prefetch(
            &app.config,
            &server,
            AppEventSender::new(tx),
            app.chat_widget.security_setup_request_id,
        );
        // The fetch owns the only sender, so channel closure proves it completed.
        assert!(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await?
                .is_none()
        );
        backend.verify().await;
        server.shutdown().await?;
        let methods = daemon.await??;
        assert!(!methods.iter().any(|method| method == "getAuthStatus"));
    }
    Ok(())
}
