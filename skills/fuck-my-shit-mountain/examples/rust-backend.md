# Example: Rust Backend Audit

## Scenario

Auditing a Rust web service that handles WebSocket connections, uses SQLite for persistence, and exposes a JSON REST API.

## Invocation

```text
Use the fuck-my-shit-mountain skill in full mode.

Audit the Rust backend service at src/.
Target release: v1.0.0-beta.
```

## Sample Finding (abbreviated from a real audit)

### Finding: Unchecked unwrap on user-controlled input in WebSocket handler

- Severity: Critical
- Confidence: High
- Category: Stability
- Status: Confirmed
- Affected area: WebSocket message handling
- Evidence:
  - File: `src/ws/handler.rs:42`
  - Function / Module: `handle_message`
  - Relevant behavior: Calls ` serde_json::from_str(&raw).unwrap()` on raw WebSocket message text without validating the message format first.
- Problem: Any malformed JSON message from a client causes a panic, crashing the WebSocket handler task. If the application uses a single-threaded async runtime or a shared task pool, this can crash the entire connection loop.
- Why it matters: An attacker can disconnect any user by sending a malformed message. This is a denial-of-service vector with no authentication requirement.
- Realistic failure scenario: Client sends `{invalid json` over the WebSocket. `from_str` returns `Err`. `unwrap` panics. The async task panics, dropping the connection and all associated state.
- Minimal fix: Replace `unwrap()` with error handling:
  ```rust
  match serde_json::from_str::<ClientMessage>(&raw) {
      Ok(msg) => process_message(msg).await,
      Err(e) => {
          warn!("Invalid message from {}: {}", peer_id, e);
          send_error(&mut conn, "invalid message format").await;
      }
  }
  ```
- Better long-term fix: Add a connection-level middleware that catches panics, logs them, and gracefully closes the connection.
- Regression test suggestion:
  ```rust
  #[tokio::test]
  async fn test_malformed_json_does_not_panic() {
      let (mut conn, _) = connect_test_client().await;
      conn.send_text("{invalid json").await;
      // Should receive error response, not disconnect
      let response = conn.receive().await;
      assert!(response.contains("error"));
      // Connection should still be alive
      conn.send_text(r#"{"type": "ping"}"#).await;
      let pong = conn.receive().await;
      assert!(pong.contains("pong"));
  }
  ```
- Estimated effort: 15 minutes

## Key Takeaways for Rust Projects

1. Search for `unwrap()`, `expect()`, `panic!()` — especially in request handlers.
2. Check unsafe blocks — each must have a safety comment.
3. Verify Send + Sync implementations on shared state.
4. Check async cancellation safety — especially in select! loops.
5. Verify Mutex/RwLock usage — are they held across await points?
6. Check for unbounded channels (tokio::mpsc::unbounded_channel).
7. Verify error types — are they using thiserror/anywhere appropriately?
8. Check for missing Clone on configuration structs.
9. Verify that Drop implementations are not async.
10. Check for correct use of Arc instead of Rc in async contexts.
